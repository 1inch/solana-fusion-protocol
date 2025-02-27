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
  orderHash: string
): Promise<void> {
  const orderHashBytes = Array.from(orderHash.match(/../g) || [], (h) =>
    parseInt(h, 16)
  );

  const escrow = findEscrowAddress(
    program.programId,
    makerKeypair.publicKey,
    orderHash
  );

  const escrowSrcAta = await splToken.getAssociatedTokenAddress(
    srcMint,
    escrow,
    true
  );

  const cancelIx = await program.methods
    .cancel(orderHashBytes)
    .accountsPartial({
      maker: makerKeypair.publicKey,
      srcMint,
      escrow,
      escrowSrcAta,
      srcTokenProgram: splToken.TOKEN_PROGRAM_ID,
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
  const orderHash = prompt("Enter order hash: ");
  const srcMint = new PublicKey(prompt("Enter src mint public key: "));

  const connection = new Connection(clusterUrl, "confirmed");
  const fusionSwap = new Program(FUSION_IDL as FusionSwap, { connection });

  const makerKeypair = await loadKeypairFromFile(makerKeypairPath);

  try {
    const escrowAddr = findEscrowAddress(
      fusionSwap.programId,
      makerKeypair.publicKey,
      orderHash
    );

    const escrowSrcAtaAddr = await splToken.getAssociatedTokenAddress(
      srcMint,
      escrowAddr,
      true
    );

    await splToken.getAccount(connection, escrowSrcAtaAddr);
    console.log(`Order exists`);
  } catch (e) {
    console.error(
      `Escrow with order hash = ${orderHash} and maker = ${makerKeypair.publicKey.toString()} does not exist`
    );
    return;
  }

  await cancel(connection, fusionSwap, makerKeypair, srcMint, orderHash);
}

main();
