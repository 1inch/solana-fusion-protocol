import * as anchor from "@coral-xyz/anchor";
import * as splToken from "@solana/spl-token";
import {
  Transaction,
  sendAndConfirmTransaction,
  PublicKey,
  LAMPORTS_PER_SOL,
} from "@solana/web3.js";
import * as splBankrunToken from "spl-token-bankrun";
import { BanksClient, Clock, ProgramTestContext } from "solana-bankrun";
import { Escrow } from "../../target/types/escrow";

export type User = {
  keypair: anchor.web3.Keypair;
  atas: {
    [tokenAddress: string]: splToken.Account;
  };
};

export type Escrow = {
  escrow: anchor.web3.PublicKey;
  order_id: number;
  ata: anchor.web3.PublicKey;
};

export class GetAmountArgs {
  initial_rate_bump: number;
  auction_start_time: number;
  auction_finish_time: number;

  constructor(properties: {
    initial_rate_bump: number;
    auction_start_time: number;
    auction_finish_time: number;
  }) {
    this.initial_rate_bump = properties.initial_rate_bump;
    this.auction_start_time = properties.auction_start_time;
    this.auction_finish_time = properties.auction_finish_time;
  }
}

export const getAmountArgsSchema = {
  struct: {
    initial_rate_bump: "u32",
    auction_start_time: "u32",
    auction_finish_time: "u32",
  },
};

export const whitelistExtraDataSchema = {
  struct: {
    whitelist: { array: { type: { array: { type: "u8", len: 32 } } } },
  },
};

export const INVALIDATOR_SIZE = 128;

export function buildEscrowTraits({
  isPartialFill = true,
  isMultipleFill = true,
}): number {
  let traits = 0;
  if (isPartialFill) {
    traits |= 1;
  }
  if (isMultipleFill) {
    traits |= 2;
  }
  return traits;
}

export function debugLog(message?: any, ...optionalParams: any[]): void {
  if (process.env.DEBUG) {
    console.log(message, ...optionalParams);
  }
}

export async function trackReceivedTokenAndTx(
  connection,
  addresses: Array<PublicKey>,
  txPromise
): Promise<BigInt[]> {
  const tokenBalancesBefore = await Promise.all(
    addresses.map(
      async (address) => await splToken.getAccount(connection, address)
    )
  );
  await txPromise();
  const tokenBalancesAfter = await Promise.all(
    addresses.map(
      async (address) => await splToken.getAccount(connection, address)
    )
  );
  return tokenBalancesAfter.map(
    (b, i) => b.amount - tokenBalancesBefore[i].amount
  );
}

export class TestState {
  alice: User;
  bob: User;
  charlie: User;
  tokens: Array<anchor.web3.PublicKey> = [];
  escrows: Array<Escrow> = [];
  defaultTraits = buildEscrowTraits({});
  order_id = 0;
  defaultSrcAmount = new anchor.BN(100);
  defaultDstAmount = new anchor.BN(30);
  defaultExpirationTime = ~~(new Date().getTime() / 1000) + 86400; // now + 1 day

  constructor() {}

  static async anchorCreate(
    provider: anchor.AnchorProvider,
    payer: anchor.web3.Keypair,
    settings: { tokensNums: number }
  ): Promise<TestState> {
    const instance = new TestState();
    instance.tokens = await createTokens(settings.tokensNums, provider, payer);
    [instance.alice as User, instance.bob as User, instance.charlie as User] =
      await createUsers(3, instance.tokens, provider, payer);

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

  static async bankrunCreate(
    provider: BanksClient,
    payer: anchor.web3.Keypair,
    usersKeypairs: Array<anchor.web3.Keypair>,
    settings: { tokensNums: number }
  ): Promise<TestState> {
    const instance = new TestState();
    instance.tokens = await createTokens(settings.tokensNums, provider, payer);
    [instance.alice as User, instance.bob as User, instance.charlie as User] =
      await createAtasUsers(usersKeypairs, instance.tokens, provider, payer);

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

  buildAccountsDataForAccept({
    taker = this.bob.keypair.publicKey,
    maker = this.alice.keypair.publicKey,
    makerReceiver = this.alice.keypair.publicKey,
    xMint = this.tokens[0],
    yMint = this.tokens[1],
    escrow = this.escrows[0].escrow,
    escrowedXTokens = this.escrows[0].ata,
    makerXTokens = this.alice.atas[this.tokens[0].toString()].address,
    makerYTokens = this.alice.atas[this.tokens[1].toString()].address,
    takerXTokens = this.bob.atas[this.tokens[0].toString()].address,
    takerYTokens = this.bob.atas[this.tokens[1].toString()].address,
    solReceiver = this.alice.keypair.publicKey,
    tokenProgram = splToken.TOKEN_PROGRAM_ID,
    takingAmountGetterProgram = null,
    makingAmountGetterProgram = null,
    predicateProgram = null,
    associatedTokenProgram = splToken.ASSOCIATED_TOKEN_PROGRAM_ID,
    systemProgram = anchor.web3.SystemProgram.programId,
  }): any {
    return {
      taker,
      maker,
      makerReceiver,
      xMint,
      yMint,
      escrow,
      escrowedXTokens,
      makerXTokens,
      makerYTokens,
      takerXTokens,
      takerYTokens,
      solReceiver,
      tokenProgram,
      takingAmountGetterProgram,
      makingAmountGetterProgram,
      predicateProgram,
      associatedTokenProgram,
      systemProgram,
    };
  }

  async initEscrow({
    escrowProgram,
    provider,
    payer,
    expirationTime = this.defaultExpirationTime,
    srcAmount = this.defaultSrcAmount,
    dstAmount = this.defaultDstAmount,
    xMint = this.tokens[0],
    yMint = this.tokens[1],
    escrow_traits = this.defaultTraits,
    makerReceiver = this.alice.keypair.publicKey,
    authorizedUser = null,
    takingAmountGetterProgram = null,
    makingAmountGetterProgram = null,
    predicateProgram = null,
    extensionHash = null,
    sol_receiver = this.alice.keypair.publicKey,
  }: {
    escrowProgram: anchor.Program<Escrow>;
    provider: anchor.AnchorProvider | BanksClient;
    payer: anchor.web3.Keypair;
    [key: string]: any;
  }): Promise<Escrow> {
    // Derive escrow address
    const [escrow] = anchor.web3.PublicKey.findProgramAddressSync(
      [
        anchor.utils.bytes.utf8.encode("escrow6"),
        this.alice.keypair.publicKey.toBuffer(),
        numberToBuffer(this.order_id, 4),
      ],
      escrowProgram.programId
    );

    // Derive order_invalidator address
    const [order_invalidator] = anchor.web3.PublicKey.findProgramAddressSync(
      [
        anchor.utils.bytes.utf8.encode("order_invalidator"),
        this.alice.keypair.publicKey.toBuffer(),
        numberToBuffer(this.order_id / INVALIDATOR_SIZE, 4),
      ],
      escrowProgram.programId
    );

    const escrowAta = await splToken.getAssociatedTokenAddress(
      xMint,
      escrow,
      true
    );

    if (provider instanceof anchor.AnchorProvider) {
      // TODO: research Bankrun native token support if needed
      if (xMint == splToken.NATIVE_MINT) {
        await prepareNativeTokens({
          amount: srcAmount,
          user: this.alice,
          provider,
          payer,
        });
      }
      if (yMint == splToken.NATIVE_MINT) {
        await prepareNativeTokens({
          amount: dstAmount,
          user: this.bob,
          provider,
          payer,
        });
      }
    }

    await escrowProgram.methods
      .initialize(
        this.order_id,
        expirationTime,
        srcAmount,
        dstAmount,
        escrow_traits,
        takingAmountGetterProgram,
        makingAmountGetterProgram,
        predicateProgram,
        extensionHash,
        sol_receiver,
        makerReceiver
      )
      .accountsPartial({
        payer: payer.publicKey,
        maker: this.alice.keypair.publicKey,
        xMint,
        yMint,
        escrow,
        orderInvalidator: order_invalidator,
        authorizedUser,
      })
      .signers([this.alice.keypair])
      .rpc();

    return { escrow, order_id: this.increaseOrderID(), ata: escrowAta };
  }

  increaseOrderID(): number {
    const order_id = this.order_id;
    this.order_id = this.order_id + 1;
    return order_id;
  }
}

async function createTokens(
  num: number,
  provider: anchor.AnchorProvider | BanksClient,
  payer: anchor.web3.Keypair
): Promise<Array<anchor.web3.PublicKey>> {
  let tokens: Array<anchor.web3.PublicKey> = [];

  const [tokenLibrary, connection] =
    provider instanceof anchor.AnchorProvider
      ? [splToken, provider.connection]
      : [splBankrunToken, provider];

  for (let i = 0; i < num; ++i) {
    tokens.push(
      await tokenLibrary.createMint(connection, payer, payer.publicKey, null, 6)
    );
  }
  tokens.push(splToken.NATIVE_MINT);
  return tokens;
}

async function createUsers(
  num: number,
  tokens: Array<anchor.web3.PublicKey>,
  provider: anchor.AnchorProvider,
  payer: anchor.web3.Keypair
): Promise<Array<User>> {
  let usersKeypairs: Array<anchor.web3.Keypair> = [];
  for (let i = 0; i < num; ++i) {
    const keypair = anchor.web3.Keypair.generate();
    usersKeypairs.push(keypair);
    await provider.connection.requestAirdrop(
      keypair.publicKey,
      1 * LAMPORTS_PER_SOL
    );
  }
  return await createAtasUsers(usersKeypairs, tokens, provider, payer);
}

async function createAtasUsers(
  usersKeypairs: Array<anchor.web3.Keypair>,
  tokens: Array<anchor.web3.PublicKey>,
  provider: anchor.AnchorProvider | BanksClient,
  payer: anchor.web3.Keypair
): Promise<Array<User>> {
  let users: Array<User> = [];

  const [tokenLibrary, connection] =
    provider instanceof anchor.AnchorProvider
      ? [splToken, provider.connection]
      : [splBankrunToken, provider];

  for (let i = 0; i < usersKeypairs.length; ++i) {
    const keypair = usersKeypairs[i];
    const atas = {};
    for (const token of tokens) {
      const pubkey = await tokenLibrary.createAssociatedTokenAccount(
        connection,
        payer,
        token,
        keypair.publicKey
      );
      atas[token.toString()] = await tokenLibrary.getAccount(
        connection,
        pubkey
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

async function mintTokens(
  token: anchor.web3.PublicKey,
  user: User,
  amount: number,
  provider: anchor.AnchorProvider | BanksClient,
  payer: anchor.web3.Keypair
) {
  const [tokenLibrary, connection] =
    provider instanceof anchor.AnchorProvider
      ? [splToken, provider.connection]
      : [splBankrunToken, provider];

  await tokenLibrary.mintTo(
    connection,
    payer,
    token,
    user.atas[token.toString()].address,
    payer,
    amount
  );
  const balance = await tokenLibrary.getAccount(
    connection,
    user.atas[token.toString()].address
  );

  debugLog(
    `User :: ${user.keypair.publicKey.toString()} :: token = ${token.toString()} :: balance = ${
      balance.amount
    }`
  );
}

async function prepareNativeTokens({ amount, user, provider, payer }) {
  const ata = user.atas[splToken.NATIVE_MINT.toString()].address;
  const wrapTransaction = new Transaction().add(
    anchor.web3.SystemProgram.transfer({
      fromPubkey: user.keypair.publicKey,
      toPubkey: ata,
      lamports: amount.toNumber(),
    }),
    splToken.createSyncNativeInstruction(ata)
  );
  await sendAndConfirmTransaction(provider.connection, wrapTransaction, [
    payer,
    user.keypair,
  ]);
}

export function numberToBuffer(n: number, bufSize: number) {
  return Buffer.from((~~n).toString(16).padStart(bufSize * 2, "0"), "hex");
}

export enum Period {
  Finality = 0,
  Withdrawal = 1,
  PublicWithdrawal = 2,
  Cancellation = 3,
  PublicCancellation = 4,
}

export const defaultPeriodDuration = 100;
export const aLittleTime = 5;

export async function setCurrentPeriod(
  context: ProgramTestContext,
  banksClient: BanksClient,
  period: Period
) {
  const currentClock = await banksClient.getClock();
  context.setClock(
    new Clock(
      currentClock.slot,
      currentClock.epochStartTimestamp,
      currentClock.epoch,
      currentClock.leaderScheduleEpoch,
      currentClock.unixTimestamp +
        BigInt(defaultPeriodDuration * period) +
        BigInt(aLittleTime)
    )
  );
}

// Bankrun test fails with "Could not find <pubkey>" error when account does not exist
export const BANKRUN_ACCOUNT_NOT_FOUND_ERROR_PREFIX = "Could not find";
