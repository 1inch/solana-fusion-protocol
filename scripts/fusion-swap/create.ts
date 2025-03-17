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
const fs = require("fs");

import FUSION_IDL from "../../target/idl/fusion_swap.json";
import { FusionSwap } from "../../target/types/fusion_swap";
import {
  calculateOrderHash,
  defaultAuctionData,
  defaultExpirationTime,
  defaultFeeConfig,
  AuctionData,
  findEscrowAddress,
  getClusterUrlEnv,
  getTokenDecimals,
  loadKeypairFromFile,
  OrderConfig,
  ReducedFeeConfig,
  ReducedOrderConfig,
} from "../utils";

const prompt = require("prompt-sync")({ sigint: true });

async function create(
  connection: Connection,
  program: Program<FusionSwap>,
  makerKeypair: Keypair,
  srcAmount: BN,
  minDstAmount: BN,
  srcMint: PublicKey,
  dstMint: PublicKey,
  orderId: number,
  expirationTime: number = defaultExpirationTime(),
  receiver: PublicKey = makerKeypair.publicKey,
  srcAssetIsNative: boolean = false,
  dstAssetIsNative: boolean = false,
  fee: ReducedFeeConfig = defaultFeeConfig,
  protocolDstAcc: PublicKey = null,
  integratorDstAcc: PublicKey = null,
  estimatedDstAmount: BN = minDstAmount,
  dutchAuctionData: AuctionData = defaultAuctionData,
  cancellationAuctionDuration: number = defaultAuctionData.duration,
  srcTokenProgram: PublicKey = splToken.TOKEN_PROGRAM_ID
): Promise<[PublicKey, PublicKey]> {
  const reducedOrderConfig: ReducedOrderConfig = {
    id: orderId,
    srcAmount,
    minDstAmount,
    estimatedDstAmount,
    expirationTime,
    srcAssetIsNative,
    dstAssetIsNative,
    fee,
    dutchAuctionData,
    cancellationAuctionDuration,
  };

  const orderConfig: OrderConfig = {
    ...reducedOrderConfig,
    srcMint,
    dstMint,
    receiver,
    fee: {
      ...fee,
      protocolDstAcc,
      integratorDstAcc,
    },
  };

  const orderHash = calculateOrderHash(orderConfig);
  console.log(`Order hash hex: ${Buffer.from(orderHash).toString("hex")}`);

  const orderConfigs = {
    full: orderConfig,
    reduced: reducedOrderConfig,
  };

  fs.writeFileSync("order.json", JSON.stringify(orderConfigs));
  console.log("Saved full and reduced order configs to order.json");

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
      lamports: srcAmount.toNumber(),
    });
    tx.add(transferIx);

    tx.add(splToken.createSyncNativeInstruction(makerNativeAta));
  }

  const createIx = await program.methods
    .create(reducedOrderConfig)
    .accountsPartial({
      maker: makerKeypair.publicKey,
      makerReceiver: receiver,
      srcMint,
      dstMint,
      escrow,
      srcTokenProgram,
      protocolDstAcc,
      integratorDstAcc,
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
    new BN(srcAmount * Math.pow(10, srcMintDecimals)),
    new BN(minDstAmount * Math.pow(10, dstMintDecimals)),
    new PublicKey(srcMint),
    new PublicKey(dstMint),
    orderId
  );

  console.log(`Escrow account address: ${escrowAddr.toString()}`);
  console.log(`Escrow src ata address: ${escrowAtaAddr.toString()}`);
}

main();
