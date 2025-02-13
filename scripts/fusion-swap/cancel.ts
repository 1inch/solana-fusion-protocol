import {
  Connection,
  Keypair,
  PublicKey,
  Transaction,
  sendAndConfirmTransaction,
} from "@solana/web3.js";
import { Program } from "@coral-xyz/anchor";
import * as splToken from "@solana/spl-token";

import FUSION_IDL from "../../target/idl/fusion_swap.json";
import { FusionSwap } from "../../target/types/fusion_swap";
import {
  findEscrowAddress,
  getClusterUrlEnv,
  loadKeypairFromFile,
} from "../utils";

const prompt = require("prompt-sync")({ sigint: true });

async function cancel(
  connection: Connection,
  program: Program<FusionSwap>,
  makerKeypair: Keypair,
  srcMint: PublicKey,
  orderId: number,
  srcTokenProgram: PublicKey = splToken.TOKEN_PROGRAM_ID
): Promise<void> {
  const escrow = findEscrowAddress(
    program.programId,
    makerKeypair.publicKey,
    orderId
  );

  const cancelIx = await program.methods
    .cancel(orderId)
    .accountsPartial({
      maker: makerKeypair.publicKey,
      srcMint,
      escrow,
      srcTokenProgram,
    })
    .signers([makerKeypair])
    .instruction();

  const tx = new Transaction().add(cancelIx);

  const signature = await sendAndConfirmTransaction(connection, tx, [
    makerKeypair,
  ]);
  console.log(`Transaction signature ${signature}`);
}

async function main() {
  const clusterUrl = getClusterUrlEnv();
  const makerKeypairPath = prompt("Enter maker keypair path: ");
  const orderId = Number(prompt("Enter order id: "));

  const connection = new Connection(clusterUrl, "confirmed");
  const fusionSwap = new Program(FUSION_IDL as FusionSwap, { connection });

  const makerKeypair = await loadKeypairFromFile(makerKeypairPath);

  try {
    const escrowAddr = findEscrowAddress(
      fusionSwap.programId,
      makerKeypair.publicKey,
      orderId
    );
    const escrowAccount = await fusionSwap.account.escrow.fetch(escrowAddr);
    console.log(JSON.stringify(escrowAccount));
  } catch (e) {
    console.error(
      `Escrow with order id = ${orderId} and maker = ${makerKeypair.publicKey.toString()} does not exist`
    );
    return;
  }

  const srcMint = new PublicKey(prompt("Enter src mint public key: "));

  await cancel(connection, fusionSwap, makerKeypair, srcMint, orderId);
}

main();
