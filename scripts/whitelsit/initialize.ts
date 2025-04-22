import {
  Connection,
  Keypair,
  Transaction,
  sendAndConfirmTransaction,
} from "@solana/web3.js";
import { Program } from "@coral-xyz/anchor";

import WHITELIST_IDL from "../../target/idl/whitelist.json";
import { Whitelist } from "../../target/types/whitelist";

import {
  findWhitelistStateAddress,
  getClusterUrlEnv,
  loadKeypairFromFile,
} from "../utils";

const prompt = require("prompt-sync")({ sigint: true });

async function initialize(
  connection: Connection,
  program: Program<Whitelist>,
  ownerKeypair: Keypair
): Promise<void> {
  const whitelistState = findWhitelistStateAddress(program.programId);

  const initializeIx = await program.methods
    .initialize()
    .accountsPartial({
      owner: ownerKeypair.publicKey,
      whitelistState,
    })
    .signers([ownerKeypair])
    .instruction();

  const tx = new Transaction().add(initializeIx);

  const signature = await sendAndConfirmTransaction(connection, tx, [
    ownerKeypair,
  ]);
  console.log(`Transaction signature ${signature}`);
}

async function main() {
  const clusterUrl = getClusterUrlEnv();

  const connection = new Connection(clusterUrl, "confirmed");
  const whitelist = new Program<Whitelist>(WHITELIST_IDL, { connection });

  const ownerKeypairPath = prompt("Enter owner keypair path: ");
  const ownerKeypair = await loadKeypairFromFile(ownerKeypairPath);

  await initialize(connection, whitelist, ownerKeypair);
}

main();
