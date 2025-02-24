import * as anchor from "@coral-xyz/anchor";
import { Whitelist } from "../../target/types/whitelist";
import { BankrunProvider } from "anchor-bankrun";

export function debugLog(message?: any, ...optionalParams: any[]): void {
  if (process.env.DEBUG) {
    console.log(message, ...optionalParams);
  }
}

export async function initializeWhitelist(
  program: anchor.Program<Whitelist>,
  owner: anchor.web3.Keypair
) {
  const [whitelistStatePDA] = anchor.web3.PublicKey.findProgramAddressSync(
    [Buffer.from("whitelist_state")],
    program.programId
  );
  try {
    await program.account.whitelistState.fetch(whitelistStatePDA);
  } catch (e) {
    const isBankrun = program.provider instanceof BankrunProvider;
    if (
      (!isBankrun &&
        e.toString().includes(ANCHOR_ACCOUNT_NOT_FOUND_ERROR_PREFIX)) ||
      (isBankrun &&
        e.toString().includes(BANKRUN_ACCOUNT_NOT_FOUND_ERROR_PREFIX))
    ) {
      // Whitelist state does not exist, initialize it
      await program.methods
        .initialize()
        .accountsPartial({
          owner: owner.publicKey,
        })
        .signers([owner])
        .rpc();
    } else {
      throw e; // Re-throw if it's a different error
    }
  }
}

// Anchor test fails with "Account does not exist <pubkey>" error when account does not exist
export const ANCHOR_ACCOUNT_NOT_FOUND_ERROR_PREFIX = "Account does not exist";
// Bankrun test fails with "Could not find <pubkey>" error when account does not exist
export const BANKRUN_ACCOUNT_NOT_FOUND_ERROR_PREFIX = "Could not find";
