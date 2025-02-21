import {
  Connection,
  Keypair,
  PublicKey,
  Transaction,
  sendAndConfirmTransaction,
} from "@solana/web3.js";
import { BN, Program } from "@coral-xyz/anchor";
import * as splToken from "@solana/spl-token";
const fs = require('fs');

import FUSION_IDL from "../../target/idl/fusion_swap.json";
import WHITELIST_IDL from "../../target/idl/whitelist.json";
import { FusionSwap } from "../../target/types/fusion_swap";
import { Whitelist } from "../../target/types/whitelist";
import {
  calculateOrderHash,
  findEscrowAddress,
  findResolverAccessAddress,
  getClusterUrlEnv,
  getTokenDecimals,
  loadKeypairFromFile,
  OrderConfig,
  ReducedOrderConfig,
} from "../utils";

const prompt = require("prompt-sync")({ sigint: true });

async function fill(
  connection: Connection,
  program: Program<FusionSwap>,
  whitelistProgramId: PublicKey,
  takerKeypair: Keypair,
  maker: PublicKey,
  amount: number,
  orderConfig: OrderConfig,
  reducedOrderConfig: ReducedOrderConfig,
): Promise<void> {
  const orderHash = calculateOrderHash(orderConfig);

  const escrow = findEscrowAddress(
    program.programId,
    maker,
    Buffer.from(orderHash)
  );
  const escrowSrcAta = await splToken.getAssociatedTokenAddress(
    orderConfig.srcMint,
    escrow,
    true
  );

  const resolverAccess = findResolverAccessAddress(
    whitelistProgramId,
    takerKeypair.publicKey
  );

  const takerSrcAta = await splToken.getAssociatedTokenAddress(
    orderConfig.srcMint,
    takerKeypair.publicKey
  );

  const srcMintDecimals = await getTokenDecimals(
    connection,
    orderConfig.srcMint
  );

  const fillIx = await program.methods
    .fill(
      reducedOrderConfig,
      new BN(amount * Math.pow(10, srcMintDecimals))
    )
    .accountsPartial({
      taker: takerKeypair.publicKey,
      resolverAccess,
      maker,
      makerReceiver: orderConfig.receiver,
      srcMint: orderConfig.srcMint,
      dstMint: orderConfig.dstMint,
      escrow,
      escrowSrcAta,
      takerSrcAta,
      protocolDstAta: orderConfig.fee.protocolDstAta,
      integratorDstAta: orderConfig.fee.integratorDstAta,
      srcTokenProgram: splToken.TOKEN_PROGRAM_ID,
      dstTokenProgram: splToken.TOKEN_PROGRAM_ID,
    })
    .signers([takerKeypair])
    .instruction();

  const tx = new Transaction().add(fillIx);

  const signature = await sendAndConfirmTransaction(connection, tx, [
    takerKeypair,
  ]);
  console.log(`Transaction signature ${signature}`);
}

async function main() {
  const clusterUrl = getClusterUrlEnv();
  const orderFilePath = prompt('Enter order config file path: ');
  const maker = new PublicKey(prompt('Enter maker public key: '));

  const orderConfigs = JSON.parse(fs.readFileSync(orderFilePath));

  const orderConfig = {
    ...orderConfigs.full,
    srcAmount: new BN(orderConfigs.full.srcAmount, "hex"),
    minDstAmount: new BN(orderConfigs.full.minDstAmount, "hex"),
    estimatedDstAmount: new BN(orderConfigs.full.estimatedDstAmount, "hex"),
    srcMint: new PublicKey(orderConfigs.full.srcMint),
    dstMint: new PublicKey(orderConfigs.full.dstMint),
    receiver: new PublicKey(orderConfigs.full.receiver)
  }
  const reducedOrderConfig = {
    ...orderConfigs.reduced,
    srcAmount: new BN(orderConfigs.reduced.srcAmount, "hex"),
    minDstAmount: new BN(orderConfigs.reduced.minDstAmount, "hex"),
    estimatedDstAmount: new BN(orderConfigs.reduced.estimatedDstAmount, "hex"),
  }

  const takerKeypairPath = prompt("Enter taker keypair path: ");
  const takerKeypair = await loadKeypairFromFile(takerKeypairPath);
  const amount = Number(prompt("Enter fill amount: "));

  const connection = new Connection(clusterUrl, "confirmed");
  const fusionSwap = new Program(FUSION_IDL as FusionSwap, { connection });
  const whitelist = new Program(WHITELIST_IDL as Whitelist, { connection });

  try {
    const orderHash = calculateOrderHash(orderConfig);

    const escrowAddr = findEscrowAddress(
      fusionSwap.programId,
      maker,
      Buffer.from(orderHash)
    );

    const escrowSrcAtaAddr = await splToken.getAssociatedTokenAddress(
      orderConfig.srcMint,
      escrowAddr,
      true
    );

    await splToken.getAccount(connection, escrowSrcAtaAddr);
    console.log(`Order exists`);
  } catch (e) {
    console.error(
      `Escrow with given order config and maker = ${maker.toString()} does not exist`
    );
    return;
  }

  await fill(
    connection,
    fusionSwap,
    whitelist.programId,
    takerKeypair,
    maker,
    amount,
    orderConfig,
    reducedOrderConfig
  );
}

main();
