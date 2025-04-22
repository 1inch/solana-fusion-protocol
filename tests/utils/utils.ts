import * as anchor from "@coral-xyz/anchor";
import * as splToken from "@solana/spl-token";
import {
  Transaction,
  sendAndConfirmTransaction,
  PublicKey,
  LAMPORTS_PER_SOL,
  TransactionSignature,
  Message,
} from "@solana/web3.js";
import * as splBankrunToken from "spl-token-bankrun";
import {
  AccountInfoBytes,
  BanksClient,
  Clock,
  ProgramTestContext,
  startAnchor,
} from "solana-bankrun";
import bs58 from "bs58";
import { FusionSwap } from "../../target/types/fusion_swap";
import { SYSTEM_PROGRAM_ID } from "@coral-xyz/anchor/dist/cjs/native/system";
import { Whitelist } from "../../target/types/whitelist";
import { BankrunProvider } from "anchor-bankrun";
import { calculateOrderHash } from "../../scripts/utils";
import { FusionSwapNative } from "../../idl/fusion_swap_native";

const WhitelistIDL = require("../../target/idl/whitelist.json");
const FusionSwapIDL = require("../../target/idl/fusion_swap.json");
const reducedOrderConfigType = FusionSwapIDL.types.find(
  (t) => t.name === "ReducedOrderConfig"
);

export type ReducedOrderConfig =
  (typeof reducedOrderConfigType)["type"]["fields"];

type FeeConfig = {
  protocolDstAcc: anchor.web3.PublicKey | null;
  integratorDstAcc: anchor.web3.PublicKey | null;
  protocolFee: number;
  integratorFee: number;
  surplusPercentage: number;
  minCancellationPremium: anchor.BN;
  maxCancellationPremium: anchor.BN;
};

type OrderConfig = ReducedOrderConfig & {
  srcMint: anchor.web3.PublicKey;
  dstMint: anchor.web3.PublicKey;
  receiver: anchor.web3.PublicKey;
  fee: FeeConfig;
};

export type User = {
  keypair: anchor.web3.Keypair;
  atas: {
    [tokenAddress: string]: splToken.Account;
  };
};

export type Escrow = {
  escrow: anchor.web3.PublicKey;
  orderConfig: OrderConfig;
  reducedOrderConfig: ReducedOrderConfig;
  ata: anchor.web3.PublicKey;
};

export type CompactFee = {
  protocolFee: number;
  integratorFee: number;
  surplus: number;
};

export function debugLog(message?: any, ...optionalParams: any[]): void {
  if (process.env.DEBUG) {
    console.log(message, ...optionalParams);
  }
}

export type Account = {
  publicKey: PublicKey;
  programId: PublicKey;
};

export async function trackReceivedTokenAndTx(
  connection,
  addresses: Array<PublicKey> | Array<Account>,
  txPromise
): Promise<BigInt[]> {
  const getAccounts = async (address) => {
    return await splToken.getAccount(
      connection,
      address instanceof PublicKey ? address : address.publicKey,
      undefined,
      address instanceof PublicKey
        ? splToken.TOKEN_PROGRAM_ID
        : address.programId
    );
  };

  const tokenBalancesBefore = await Promise.all(addresses.map(getAccounts));
  await txPromise();
  const tokenBalancesAfter = await Promise.all(addresses.map(getAccounts));
  return tokenBalancesAfter.map(
    (b, i) => b.amount - tokenBalancesBefore[i].amount
  );
}

const DEFAULT_AIRDROPINFO = {
  lamports: 1 * LAMPORTS_PER_SOL,
  data: Buffer.alloc(0),
  owner: SYSTEM_PROGRAM_ID,
  executable: false,
};

const DEFAULT_STARTANCHOR = {
  path: ".",
  extraPrograms: [],
  accounts: undefined,
  computeMaxUnits: undefined,
  transactionAccountLockLimit: undefined,
  deactivateFeatures: undefined,
};

export class TestState {
  alice: User;
  bob: User;
  charlie: User;
  dave: User;
  tokens: Array<anchor.web3.PublicKey> = [];
  escrows: Array<Escrow> = [];
  order_id = 0;
  defaultSrcAmount = new anchor.BN(100);
  defaultDstAmount = new anchor.BN(30);
  defaultExpirationTime = ~~(new Date().getTime() / 1000) + 86400; // now + 1 day
  auction = {
    startTime: 0xffffffff - 32000, // default auction start in the far far future and order use default formula
    duration: 32000,
    initialRateBump: 0,
    pointsAndTimeDeltas: [],
  };

  constructor() {}

  static async anchorCreate(
    provider: anchor.AnchorProvider,
    payer: anchor.web3.Keypair,
    settings: { tokensNums: number }
  ): Promise<TestState> {
    const instance = new TestState();
    instance.tokens = await createTokens(settings.tokensNums, provider, payer);
    instance.tokens.push(splToken.NATIVE_MINT);
    [
      instance.alice as User,
      instance.bob as User,
      instance.charlie as User,
      instance.dave as User,
    ] = await createUsers(4, instance.tokens, provider, payer);
    // Create whitelisted account for Bob
    const whitelistProgram = anchor.workspace
      .Whitelist as anchor.Program<Whitelist>;
    await createWhitelistedAccount(
      whitelistProgram,
      instance.bob.keypair,
      payer
    );

    await mintTokens(
      instance.tokens[0],
      instance.alice,
      100_000_000,
      provider,
      payer
    );
    await mintTokens(
      instance.tokens[1],
      instance.bob,
      100_000_000,
      provider,
      payer
    );
    await mintTokens(
      instance.tokens[1],
      instance.charlie,
      100_000_000,
      provider,
      payer
    );
    return instance;
  }

  static async bankrunContext(
    userKeyPairs: anchor.web3.Keypair[],
    params?: typeof DEFAULT_STARTANCHOR,
    airdropInfo?: AccountInfoBytes
  ): Promise<ProgramTestContext> {
    // Fill settings with default values and rewrite some values with provided
    airdropInfo = { ...DEFAULT_AIRDROPINFO, ...airdropInfo };
    params = { ...DEFAULT_STARTANCHOR, ...params };

    return await startAnchor(
      params.path,
      params.extraPrograms,
      params.accounts ||
        userKeyPairs.map((u) => ({
          address: u.publicKey,
          info: airdropInfo,
        })),
      params.computeMaxUnits,
      params.transactionAccountLockLimit,
      params.deactivateFeatures
    );
  }

  static async bankrunCreate(
    context: ProgramTestContext,
    payer: anchor.web3.Keypair,
    usersKeypairs: Array<anchor.web3.Keypair>,
    settings: { tokensNums: number }
  ): Promise<TestState> {
    const provider = context.banksClient;

    const instance = new TestState();
    instance.tokens = await createTokens(settings.tokensNums, provider, payer);
    instance.tokens.push(splToken.NATIVE_MINT);
    [
      instance.alice as User,
      instance.bob as User,
      instance.charlie as User,
      instance.dave as User,
    ] = await createAtasUsers(usersKeypairs, instance.tokens, provider, payer);
    // Create whitelisted account for Bob
    const whitelistProgram = new anchor.Program<Whitelist>(
      WhitelistIDL,
      new BankrunProvider(context)
    );
    await createWhitelistedAccount(
      whitelistProgram,
      instance.bob.keypair,
      payer
    );

    await mintTokens(
      instance.tokens[0],
      instance.alice,
      100_000_000,
      provider,
      payer
    );
    await mintTokens(
      instance.tokens[1],
      instance.bob,
      100_000_000,
      provider,
      payer
    );
    await mintTokens(
      instance.tokens[1],
      instance.charlie,
      100_000_000,
      provider,
      payer
    );
    return instance;
  }

  buildAccountsDataForFill({
    taker = this.bob.keypair.publicKey,
    maker = this.alice.keypair.publicKey,
    makerReceiver = this.alice.keypair.publicKey,
    srcMint = this.tokens[0],
    dstMint = this.tokens[1],
    escrow = this.escrows[0].escrow,
    escrowSrcAta = this.escrows[0].ata,
    makerDstAta = this.alice.atas[this.tokens[1].toString()].address,
    takerSrcAta = this.bob.atas[this.tokens[0].toString()].address,
    takerDstAta = this.bob.atas[this.tokens[1].toString()].address,
    protocolDstAcc = null,
    integratorDstAcc = null,
    srcTokenProgram = splToken.TOKEN_PROGRAM_ID,
    dstTokenProgram = splToken.TOKEN_PROGRAM_ID,
  }): any {
    return {
      taker,
      maker,
      makerReceiver,
      srcMint,
      dstMint,
      escrow,
      escrowSrcAta,
      makerDstAta,
      takerSrcAta,
      takerDstAta,
      protocolDstAcc,
      integratorDstAcc,
      srcTokenProgram,
      dstTokenProgram,
    };
  }

  async createEscrow({
    escrowProgram,
    provider,
    payer,
    orderConfig,
    srcTokenProgram = splToken.TOKEN_PROGRAM_ID,
  }: {
    escrowProgram:
      | anchor.Program<FusionSwap>
      | anchor.Program<FusionSwapNative>;
    provider: anchor.AnchorProvider | BanksClient;
    payer: anchor.web3.Keypair;
    orderConfig?: Partial<OrderConfig>;
    srcTokenProgram?: anchor.web3.PublicKey;
  }): Promise<Escrow> {
    orderConfig = this.orderConfig(orderConfig);

    // Derive escrow address
    const [escrow] = anchor.web3.PublicKey.findProgramAddressSync(
      [
        anchor.utils.bytes.utf8.encode("escrow"),
        this.alice.keypair.publicKey.toBuffer(),
        calculateOrderHash(orderConfig),
      ],
      escrowProgram.programId
    );

    const escrowAta = await splToken.getAssociatedTokenAddress(
      orderConfig.srcMint,
      escrow,
      true,
      srcTokenProgram
    );

    if (
      orderConfig.srcMint == splToken.NATIVE_MINT &&
      !orderConfig.srcAssetIsNative
    ) {
      await prepareNativeTokens({
        amount: orderConfig.srcAmount,
        user: this.alice,
        provider,
        payer,
      });
    }
    if (
      orderConfig.dstMint == splToken.NATIVE_MINT &&
      !orderConfig.dstAssetIsNative
    ) {
      await prepareNativeTokens({
        amount: orderConfig.minDstAmount,
        user: this.bob,
        provider,
        payer,
      });
    }

    const txBuilder = escrowProgram.methods
      .create(orderConfig as ReducedOrderConfig)
      .accountsPartial({
        maker: this.alice.keypair.publicKey,
        makerReceiver: orderConfig.receiver,
        srcMint: orderConfig.srcMint,
        dstMint: orderConfig.dstMint,
        protocolDstAcc: orderConfig.fee.protocolDstAcc,
        integratorDstAcc: orderConfig.fee.integratorDstAcc,
        escrow,
        srcTokenProgram,
        makerSrcAta: orderConfig.srcAssetIsNative ? null : undefined,
      })
      .signers([this.alice.keypair]);

    if (provider instanceof anchor.AnchorProvider) {
      const tx = await txBuilder.transaction();

      await sendAndConfirmTransaction(provider.connection, tx, [
        payer,
        this.alice.keypair,
      ]);
    } else {
      await txBuilder.rpc();
    }

    return {
      escrow,
      orderConfig,
      reducedOrderConfig: orderConfig as ReducedOrderConfig,
      ata: escrowAta,
    };
  }

  orderConfig(params: Partial<OrderConfig> = {}): OrderConfig {
    return {
      id: this.order_id++,
      srcAmount: this.defaultSrcAmount,
      minDstAmount: this.defaultDstAmount,
      estimatedDstAmount: this.defaultDstAmount,
      expirationTime: this.defaultExpirationTime,
      srcAssetIsNative: false,
      dstAssetIsNative: false,
      receiver: this.alice.keypair.publicKey,
      dutchAuctionData: this.auction,
      cancellationAuctionDuration: 0,
      srcMint: this.tokens[0],
      dstMint: this.tokens[1],
      ...params,
      fee: {
        protocolDstAcc: null,
        integratorDstAcc: null,
        protocolFee: 0,
        integratorFee: 0,
        surplusPercentage: 0,
        minCancellationPremium: new anchor.BN(0),
        maxCancellationPremium: new anchor.BN(0),
        ...(params.fee ?? {}),
      },
    };
  }
}

let tokensCounter = 0;
export async function createTokens(
  num: number,
  provider: anchor.AnchorProvider | BanksClient,
  payer: anchor.web3.Keypair,
  programId = splToken.TOKEN_PROGRAM_ID
): Promise<Array<anchor.web3.PublicKey>> {
  let tokens: Array<anchor.web3.PublicKey> = [];

  const [tokenLibrary, connection, extraArgs] =
    provider instanceof anchor.AnchorProvider
      ? [splToken, provider.connection, [undefined, programId]]
      : [splBankrunToken, provider, [programId]];

  for (let i = 0; i < num; ++i, ++tokensCounter) {
    const keypair = anchor.web3.Keypair.fromSeed(
      new Uint8Array(32).fill(tokensCounter + 101)
    );
    tokens.push(
      await tokenLibrary.createMint(
        connection,
        payer,
        payer.publicKey,
        null,
        6,
        keypair,
        ...extraArgs
      )
    );
  }
  return tokens;
}

let usersCounter = 0;
async function createUsers(
  num: number,
  tokens: Array<anchor.web3.PublicKey>,
  provider: anchor.AnchorProvider | BanksClient,
  payer: anchor.web3.Keypair
): Promise<Array<User>> {
  let usersKeypairs: Array<anchor.web3.Keypair> = [];
  for (let i = 0; i < num; ++i, ++usersCounter) {
    const keypair = anchor.web3.Keypair.fromSeed(
      new Uint8Array(32).fill(usersCounter)
    );
    usersKeypairs.push(keypair);
    if (provider instanceof anchor.AnchorProvider) {
      await provider.connection.requestAirdrop(
        keypair.publicKey,
        1 * LAMPORTS_PER_SOL
      );
    }
  }
  return await createAtasUsers(usersKeypairs, tokens, provider, payer);
}

export async function initializeWhitelist(
  program: anchor.Program<Whitelist>,
  owner: anchor.web3.Keypair
) {
  const [whitelistStatePDA] = anchor.web3.PublicKey.findProgramAddressSync(
    [Buffer.from("whitelist_state")],
    program.programId
  );
  try {
    await program.account.whitelistState.fetch(whitelistStatePDA);
  } catch (e) {
    const isBankrun = program.provider instanceof BankrunProvider;
    if (
      (!isBankrun &&
        e.toString().includes(ANCHOR_ACCOUNT_NOT_FOUND_ERROR_PREFIX)) ||
      (isBankrun &&
        e.toString().includes(BANKRUN_ACCOUNT_NOT_FOUND_ERROR_PREFIX))
    ) {
      // Whitelist state does not exist, initialize it
      await program.methods
        .initialize()
        .accountsPartial({
          owner: owner.publicKey,
        })
        .signers([owner])
        .rpc();
    } else {
      throw e; // Re-throw if it's a different error
    }
  }
}

export async function createWhitelistedAccount(
  program: anchor.Program<Whitelist>,
  user: anchor.web3.Keypair,
  owner: anchor.web3.Keypair
) {
  // Initialize the whitelist state with the payer as owner
  await initializeWhitelist(program, owner);
  // Register the user
  await program.methods
    .register(user.publicKey)
    .accountsPartial({
      owner: owner.publicKey,
    })
    .signers([owner])
    .rpc();
}

export async function removeWhitelistedAccount(
  user: anchor.web3.Keypair,
  owner: anchor.web3.Keypair
) {
  const program = anchor.workspace.Whitelist as anchor.Program<Whitelist>;
  // Deregister the user
  await program.methods
    .deregister(user.publicKey)
    .accountsPartial({
      owner: owner.publicKey,
    })
    .signers([owner])
    .rpc();
}

export async function createAtasUsers(
  usersKeypairs: Array<anchor.web3.Keypair>,
  tokens: Array<anchor.web3.PublicKey>,
  provider: anchor.AnchorProvider | BanksClient,
  payer: anchor.web3.Keypair,
  tokenProgram = splToken.TOKEN_PROGRAM_ID
): Promise<Array<User>> {
  let users: Array<User> = [];

  const [tokenLibrary, connection, extraArgs] =
    provider instanceof anchor.AnchorProvider
      ? [splToken, provider.connection, [undefined, tokenProgram]]
      : [splBankrunToken, provider, [tokenProgram]];

  for (let i = 0; i < usersKeypairs.length; ++i) {
    const keypair = usersKeypairs[i];
    const atas = {};
    for (const token of tokens) {
      const pubkey = await tokenLibrary.createAssociatedTokenAccount(
        connection,
        payer,
        token,
        keypair.publicKey,
        ...extraArgs
      );
      atas[token.toString()] = await tokenLibrary.getAccount(
        connection,
        pubkey,
        undefined,
        tokenProgram
      );
      debugLog(
        `User_${i} :: token = ${token.toString()} :: ata = ${atas[
          token.toString()
        ].address.toBase58()}`
      );
    }
    users.push({ keypair, atas });
    debugLog(`User_${i} ::`, users[i].keypair.publicKey.toString(), "\n");
  }
  return users;
}

export async function mintTokens(
  token: anchor.web3.PublicKey,
  user: User,
  amount: number,
  provider: anchor.AnchorProvider | BanksClient,
  payer: anchor.web3.Keypair,
  tokenProgram = splToken.TOKEN_PROGRAM_ID
) {
  const [tokenLibrary, connection, extraArgs] =
    provider instanceof anchor.AnchorProvider
      ? [splToken, provider.connection, [undefined, tokenProgram]]
      : [splBankrunToken, provider, [tokenProgram]];

  await tokenLibrary.mintTo(
    connection,
    payer,
    token,
    user.atas[token.toString()].address,
    payer,
    amount,
    [],
    ...extraArgs
  );
  const balance = await tokenLibrary.getAccount(
    connection,
    user.atas[token.toString()].address,
    undefined,
    tokenProgram
  );

  debugLog(
    `User :: ${user.keypair.publicKey.toString()} :: token = ${token.toString()} :: balance = ${
      balance.amount
    }`
  );
}

async function prepareNativeTokens({
  amount,
  user,
  provider,
  payer,
}: {
  amount: anchor.BN;
  user: User;
  provider: anchor.AnchorProvider | BanksClient;
  payer: anchor.web3.Keypair;
}) {
  const ata = user.atas[splToken.NATIVE_MINT.toString()].address;
  const wrapTransaction = new Transaction().add(
    anchor.web3.SystemProgram.transfer({
      fromPubkey: user.keypair.publicKey,
      toPubkey: ata,
      lamports: amount.toNumber(),
    }),
    splToken.createSyncNativeInstruction(ata)
  );
  if (provider instanceof anchor.AnchorProvider) {
    await sendAndConfirmTransaction(provider.connection, wrapTransaction, [
      payer,
      user.keypair,
    ]);
  } else {
    wrapTransaction.recentBlockhash = (await provider.getLatestBlockhash())[0];
    wrapTransaction.sign(payer);
    wrapTransaction.sign(user.keypair);
    await provider.processTransaction(wrapTransaction);
  }
}

export async function setCurrentTime(
  context: ProgramTestContext,
  time: number
): Promise<void> {
  const currentClock = await context.banksClient.getClock();
  context.setClock(
    new Clock(
      currentClock.slot,
      currentClock.epochStartTimestamp,
      currentClock.epoch,
      currentClock.leaderScheduleEpoch,
      BigInt(time)
    )
  );
}

type TxInfoInstruction = {
  data: string | Uint8Array;
  accountsIndexes: number[];
};

class TxInfo {
  label: string;
  instructions: TxInfoInstruction[];
  length: number;
  computeUnits: number;

  constructor({
    label = "",
    instructions = [],
    length = 0,
    computeUnits = 0,
  }: {
    label: string;
    instructions: TxInfoInstruction[];
    length: number;
    computeUnits: number;
  }) {
    this.label = label;
    this.instructions = instructions;
    this.length = length;
    this.computeUnits = computeUnits;
  }

  toString() {
    return `Tx ${this.label}: ${this.length} bytes, ${
      this.computeUnits
    } compute units\n${this.instructions
      .map(
        (ix, i) =>
          `\tinst ${i}: ${ix.data.length} bytes + ${ix.accountsIndexes.length} accounts \n`
      )
      .join("")}`;
  }
}

export async function printTxCosts(
  label: string,
  txSignature: TransactionSignature,
  connection: anchor.web3.Connection
) {
  await waitForNewBlock(connection, 1);
  const tx = await connection.getTransaction(txSignature, {
    commitment: "confirmed",
    maxSupportedTransactionVersion: 0,
  });

  const serializedMessage = tx.transaction.message.serialize();
  const signaturesSize = tx.transaction.signatures.length * 64;
  const totalSize = serializedMessage.length + 1 + signaturesSize; // 1 byte for numSignatures

  const txInfo = new TxInfo({
    label,
    length: totalSize,
    computeUnits: tx.meta.computeUnitsConsumed,
    instructions: [],
  });

  if (tx.transaction.message instanceof Message) {
    tx.transaction.message.instructions.forEach((ix) => {
      txInfo.instructions.push({
        data: bs58.decode(ix.data),
        accountsIndexes: ix.accounts,
      });
    });
  } else {
    tx.transaction.message.compiledInstructions.forEach((ix) => {
      txInfo.instructions.push({
        data: ix.data,
        accountsIndexes: ix.accountKeyIndexes,
      });
    });
  }

  console.log(txInfo.toString());
}

export async function waitForNewBlock(
  connection: anchor.web3.Connection,
  targetHeight: number
): Promise<void> {
  debugLog(`Waiting for ${targetHeight} new blocks`);
  return new Promise(async (resolve: any) => {
    // Get the last valid block height of the blockchain
    const { lastValidBlockHeight } = await connection.getLatestBlockhash();

    // Set an interval to check for new blocks every 1000ms
    const intervalId = setInterval(async () => {
      // Get the new valid block height
      const { lastValidBlockHeight: newValidBlockHeight } =
        await connection.getLatestBlockhash();

      // Check if the new valid block height is greater than the target block height
      if (newValidBlockHeight > lastValidBlockHeight + targetHeight) {
        // If the target block height is reached, clear the interval and resolve the promise
        clearInterval(intervalId);
        resolve();
      }
    }, 1000);
  });
}

// Anchor test fails with "Account does not exist <pubkey>" error when account does not exist
export const ANCHOR_ACCOUNT_NOT_FOUND_ERROR_PREFIX = "Account does not exist";
// Bankrun test fails with "Could not find <pubkey>" error when account does not exist
export const BANKRUN_ACCOUNT_NOT_FOUND_ERROR_PREFIX = "Could not find";
