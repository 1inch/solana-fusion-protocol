import * as anchor from "@coral-xyz/anchor";
import * as splToken from "@solana/spl-token";
import {
  Transaction,
  sendAndConfirmTransaction,
  PublicKey,
  LAMPORTS_PER_SOL,
} from "@solana/web3.js";
import * as splBankrunToken from "spl-token-bankrun";
import { BanksClient } from "solana-bankrun";
import { FusionSwap } from "../../target/types/fusion_swap";

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

export function buildEscrowTraits({
  isPartialFill = true,
  isNativeDstAsset = false,
}): number {
  let traits = 0;
  if (isPartialFill) {
    traits |= 1;
  }
  if (isNativeDstAsset) {
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
    tokenProgram = splToken.TOKEN_PROGRAM_ID,
    associatedTokenProgram = splToken.ASSOCIATED_TOKEN_PROGRAM_ID,
    systemProgram = anchor.web3.SystemProgram.programId,
    protocolDstAta = null,
    integratorDstAta = null,
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
      tokenProgram,
      associatedTokenProgram,
      systemProgram,
      protocolDstAta,
      integratorDstAta,
    };
  }

  async initEscrow({
    escrowProgram,
    provider,
    payer,
    expirationTime = this.defaultExpirationTime,
    srcAmount = this.defaultSrcAmount,
    dstAmount = this.defaultDstAmount,
    srcMint = this.tokens[0],
    dstMint = this.tokens[1],
    allowPartialFills = true,
    useNativeDstAsset = false,
    makerReceiver = this.alice.keypair.publicKey,
    authorizedUser = null,
    compactFees = new anchor.BN(0),
    protocolDstAta = null,
    integratorDstAta = null,
    estimatedDstAmount = this.defaultDstAmount,
  }: {
    escrowProgram: anchor.Program<FusionSwap>;
    provider: anchor.AnchorProvider | BanksClient;
    payer: anchor.web3.Keypair;
    [key: string]: any;
  }): Promise<Escrow> {
    // Derive escrow address
    const [escrow] = anchor.web3.PublicKey.findProgramAddressSync(
      [
        anchor.utils.bytes.utf8.encode("escrow"),
        this.alice.keypair.publicKey.toBuffer(),
        numberToBuffer(this.order_id, 4),
      ],
      escrowProgram.programId
    );

    const escrowAta = await splToken.getAssociatedTokenAddress(
      srcMint,
      escrow,
      true
    );

    if (provider instanceof anchor.AnchorProvider) {
      // TODO: research Bankrun native token support if needed
      if (srcMint == splToken.NATIVE_MINT) {
        await prepareNativeTokens({
          amount: srcAmount,
          user: this.alice,
          provider,
          payer,
        });
      }
      if (dstMint == splToken.NATIVE_MINT) {
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
        buildEscrowTraits({
          isPartialFill: allowPartialFills,
          isNativeDstAsset: useNativeDstAsset,
        }),
        makerReceiver,
        compactFees,
        protocolDstAta,
        integratorDstAta,
        estimatedDstAmount
      )
      .accountsPartial({
        maker: this.alice.keypair.publicKey,
        srcMint,
        dstMint,
        escrow,
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

// Bankrun test fails with "Could not find <pubkey>" error when account does not exist
export const BANKRUN_ACCOUNT_NOT_FOUND_ERROR_PREFIX = "Could not find";
