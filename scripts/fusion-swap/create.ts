import {
  Connection,
  Keypair,
  PublicKey,
  SystemProgram,
  Transaction,
  sendAndConfirmTransaction,
} from "@solana/web3.js";
import { BN, Program } from "@coral-xyz/anchor";
import * as splToken from "@solana/spl-token";

import FUSION_IDL from "../../target/idl/fusion_swap.json";
import { FusionSwap } from "../../target/types/fusion_swap";
import {
  defaultAuctionData,
  defaultExpirationTime,
  defaultFeeConfig,
  DutchAuctionData,
  FeeConfig,
  findEscrowAddress,
  getClusterUrlEnv,
  getTokenDecimals,
  loadKeypairFromFile,
} from "../utils";
import { sha256 } from "@noble/hashes/sha256";

const prompt = require("prompt-sync")({ sigint: true });

async function create(
  connection: Connection,
  program: Program<FusionSwap>,
  makerKeypair: Keypair,
  srcAmount: number,
  minDstAmount: number,
  srcMint: PublicKey,
  dstMint: PublicKey,
  orderId: number,
  expirationTime: number = defaultExpirationTime(),
  receiver: PublicKey = makerKeypair.publicKey,
  nativeDstAsset: boolean = false,
  fees: FeeConfig = defaultFeeConfig,
  protocolDstAta: PublicKey = null,
  integratorDstAta: PublicKey = null,
  estimatedDstAmount: number = minDstAmount,
  dutchAuctionData: DutchAuctionData = defaultAuctionData,
  srcTokenProgram: PublicKey = splToken.TOKEN_PROGRAM_ID
): Promise<[PublicKey, PublicKey]> {
  const orderConfig = {
    orderId,
    maker: makerKeypair.publicKey,
    srcAmount,
    minDstAmount,
    expirationTime,
    receiver,
    nativeDstAsset,
    fees,
    dutchAuctionData,
    srcMint,
    dstMint,
  };

  const orderHash = sha256(
    program.coder.types.encode("orderConfig", orderConfig)
  );

  const escrow = findEscrowAddress(
    program.programId,
    makerKeypair.publicKey,
    Buffer.from(orderHash)
  );
  const escrowAta = await splToken.getAssociatedTokenAddress(
    srcMint,
    escrow,
    true
  );

  let tx = new Transaction();

  if (srcMint == splToken.NATIVE_MINT) {
    // Wrap SOL to wSOL
    const makerNativeAta = await splToken.getAssociatedTokenAddress(
      splToken.NATIVE_MINT,
      makerKeypair.publicKey
    );

    const transferIx = SystemProgram.transfer({
      fromPubkey: makerKeypair.publicKey,
      toPubkey: makerNativeAta,
      lamports: srcAmount,
    });
    tx.add(transferIx);

    tx.add(splToken.createSyncNativeInstruction(makerNativeAta));
  }

  const createIx = await program.methods
    .create({
      id: orderId,
      srcAmount: new BN(srcAmount),
      minDstAmount: new BN(minDstAmount),
      estimatedDstAmount: new BN(estimatedDstAmount),
      expirationTime,
      nativeDstAsset,
      receiver,
      fee: fees,
      dutchAuctionData,
      srcMint: srcMint,
      dstMint: dstMint,
    })
    .accountsPartial({
      maker: makerKeypair.publicKey,
      srcMint,
      dstMint,
      escrow,
      srcTokenProgram,
      protocolDstAta,
      integratorDstAta,
    })
    .signers([makerKeypair])
    .instruction();

  tx.add(createIx);

  const signature = await sendAndConfirmTransaction(connection, tx, [
    makerKeypair,
  ]);
  console.log(`Transaction signature ${signature}`);

  return [escrow, escrowAta];
}

async function main() {
  const clusterUrl = getClusterUrlEnv();
  const makerKeypairPath = prompt("Enter maker keypair path: ");
  const srcMint = new PublicKey(prompt("Enter src mint public key: "));
  const dstMint = new PublicKey(prompt("Enter dst mint public key: "));
  const srcAmount = Number(prompt("Enter src amount: "));
  const minDstAmount = Number(prompt("Enter min dst amount: "));
  const orderId = Number(prompt("Enter order id: "));

  const connection = new Connection(clusterUrl, "confirmed");
  const fusionSwap = new Program(FUSION_IDL as FusionSwap, { connection });

  const makerKeypair = await loadKeypairFromFile(makerKeypairPath);

  const srcMintDecimals = await getTokenDecimals(connection, srcMint);
  const dstMintDecimals = await getTokenDecimals(connection, dstMint);

  const [escrowAddr, escrowAtaAddr] = await create(
    connection,
    fusionSwap,
    makerKeypair,
    srcAmount * Math.pow(10, srcMintDecimals),
    minDstAmount * Math.pow(10, dstMintDecimals),
    new PublicKey(srcMint),
    new PublicKey(dstMint),
    orderId
  );

  console.log(`Escrow account address: ${escrowAddr.toString()}`);
  console.log(`Escrow src ata address: ${escrowAtaAddr.toString()}`);
}

main();
