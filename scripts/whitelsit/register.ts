import {
  Connection,
  Keypair,
  PublicKey,
  Transaction,
  sendAndConfirmTransaction,
} from "@solana/web3.js";
import { Program } from "@coral-xyz/anchor";

import WHITELIST_IDL from "../../target/idl/whitelist.json";
import { Whitelist } from "../../target/types/whitelist";

import {
  findResolverAccessAddress,
  findWhitelistStateAddress,
  getClusterUrlEnv,
  loadKeypairFromFile,
} from "../utils";

const prompt = require("prompt-sync")({ sigint: true });

async function register(
  connection: Connection,
  program: Program<Whitelist>,
  authorityKeypair: Keypair,
  user: PublicKey
): Promise<void> {
  const whitelistState = findWhitelistStateAddress(program.programId);
  const resolverAccess = findResolverAccessAddress(program.programId, user);

  const registerIx = await program.methods
    .register(user)
    .accountsPartial({
      authority: authorityKeypair.publicKey,
      whitelistState,
      resolverAccess,
    })
    .signers([authorityKeypair])
    .instruction();

  const tx = new Transaction().add(registerIx);

  const signature = await sendAndConfirmTransaction(connection, tx, [
    authorityKeypair,
  ]);
  console.log(`Transaction signature ${signature}`);
}

async function main() {
  const clusterUrl = getClusterUrlEnv();

  const connection = new Connection(clusterUrl, "confirmed");
  const whitelist = new Program<Whitelist>(WHITELIST_IDL, { connection });

  const authorityKeypairPath = prompt("Enter authority keypair path: ");
  const authorityKeypair = await loadKeypairFromFile(authorityKeypairPath);
  const user = new PublicKey(prompt("Enter user public key: "));

  await register(connection, whitelist, authorityKeypair, user);
}

main();
