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
  findWhitelistStateAddress,
  getClusterUrlEnv,
  loadKeypairFromFile,
  prompt_,
} from "../utils";

async function setAuthority(
  connection: Connection,
  program: Program<Whitelist>,
  currentAuthorityKeypair: Keypair,
  newAuthority: PublicKey
): Promise<void> {
  const whitelistState = findWhitelistStateAddress(program.programId);

  const setAuthorityIx = await program.methods
    .setAuthority(newAuthority)
    .accountsPartial({
      currentAuthority: currentAuthorityKeypair.publicKey,
      whitelistState,
    })
    .signers([currentAuthorityKeypair])
    .instruction();

  const tx = new Transaction().add(setAuthorityIx);

  const signature = await sendAndConfirmTransaction(connection, tx, [
    currentAuthorityKeypair,
  ]);
  console.log(`Transaction signature ${signature}`);
  console.log("New whitelist authority: " + newAuthority.toString());
}

async function main() {
  const clusterUrl = getClusterUrlEnv();

  const connection = new Connection(clusterUrl, "confirmed");
  const whitelist = new Program<Whitelist>(WHITELIST_IDL, { connection });

  const currentAuthorityKeypairPath = prompt_(
    "authority-kp",
    "Enter current authority keypair path: "
  );
  const currentAuthorityKeypair = await loadKeypairFromFile(
    currentAuthorityKeypairPath
  );
  const newAuthority = new PublicKey(
    prompt_("new-authority", "Enter new authority public key: ")
  );

  const whitelistState = findWhitelistStateAddress(whitelist.programId);
  const state = await whitelist.account.whitelistState.fetch(whitelistState);
  console.log("Program: " + whitelist.programId.toString());
  console.log("Whitelist state: " + whitelistState.toString());
  console.log("Current authority: " + state.authority.toString());

  await setAuthority(
    connection,
    whitelist,
    currentAuthorityKeypair,
    newAuthority
  );
}

main();
