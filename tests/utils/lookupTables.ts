import {
  TransactionInstruction,
  AddressLookupTableAccount,
  Keypair,
  AddressLookupTableProgram,
  Connection,
  PublicKey,
  TransactionMessage,
  VersionedTransaction,
} from "@solana/web3.js";

export async function initializeLookupTable(
  payer: Keypair,
  connection: Connection,
  addresses: PublicKey[]
): Promise<PublicKey> {
  const slot = await connection.getSlot();

  const [lookupTableInst, lookupTableAddress] =
    AddressLookupTableProgram.createLookupTable({
      authority: payer.publicKey,
      payer: payer.publicKey,
      recentSlot: slot - 1,
    });

  const extendInstruction = AddressLookupTableProgram.extendLookupTable({
    payer: payer.publicKey,
    authority: payer.publicKey,
    lookupTable: lookupTableAddress,
    addresses: addresses.slice(0, 30), // 30 is a maximum limit of accounts in ALT
  });

  await sendV0Transaction(connection, payer, [
    lookupTableInst,
    extendInstruction,
  ]);
  await waitForNewBlock(connection, 1);

  return lookupTableAddress;
}

export async function sendV0Transaction(
  connection: Connection,
  payer: Keypair,
  instructions: TransactionInstruction[],
  lookupTableAccounts?: AddressLookupTableAccount[]
): Promise<string> {
  // Get the latest blockhash and last valid block height
  const { lastValidBlockHeight, blockhash } =
    await connection.getLatestBlockhash();

  // Create a new transaction message with the provided instructions
  const messageV0 = new TransactionMessage({
    payerKey: payer.publicKey, // The payer (i.e., the account that will pay for the transaction fees)
    recentBlockhash: blockhash, // The blockhash of the most recent block
    instructions, // The instructions to include in the transaction
  }).compileToV0Message(lookupTableAccounts ? lookupTableAccounts : undefined);

  // Create a new transaction object with the message
  const transaction = new VersionedTransaction(messageV0);

  // Sign the transaction with the payer's keypair
  transaction.sign([payer]);

  // Send the transaction to the cluster
  const txid = await connection.sendTransaction(transaction);

  // Confirm the transaction
  await connection.confirmTransaction(
    {
      blockhash: blockhash,
      lastValidBlockHeight: lastValidBlockHeight,
      signature: txid,
    },
    "finalized"
  );

  return txid;
}

export async function waitForNewBlock(
  connection: Connection,
  targetHeight: number
): Promise<void> {
  return new Promise(async (resolve: any) => {
    // Get the last valid block height of the blockchain
    const { lastValidBlockHeight } = await connection.getLatestBlockhash();

    // Set an interval to check for new blocks every 1000ms
    const intervalId = setInterval(async () => {
      // Get the new valid block height
      const { lastValidBlockHeight: newValidBlockHeight } =
        await connection.getLatestBlockhash();

      // Check if the new valid block height is greater than the target block height
      if (newValidBlockHeight > lastValidBlockHeight + targetHeight) {
        // If the target block height is reached, clear the interval and resolve the promise
        clearInterval(intervalId);
        resolve();
      }
    }, 1000);
  });
}
