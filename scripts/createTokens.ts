import { Connection, Keypair, PublicKey } from "@solana/web3.js";
import * as splToken from "@solana/spl-token";
import { getClusterUrlEnv, loadKeypairFromFile } from "./utils";
const prompt = require("prompt-sync")({ sigint: true });

const PAYER_KEYPAIR_PATH = "~/.config/solana/id.json"; // Default keypair
const AMOUNT_TO_MINT = 1_000_000;
const DECIMALS = 9;

async function createMint(
  connection: Connection,
  payer: Keypair
): Promise<PublicKey> {
  return await splToken.createMint(
    connection,
    payer,
    payer.publicKey,
    null,
    DECIMALS
  );
}

async function mintTo(
  connection: Connection,
  payer: Keypair,
  mint: PublicKey,
  owner: PublicKey,
  amount: number
): Promise<PublicKey> {
  const tokenAccount = await splToken.createAssociatedTokenAccount(
    connection,
    payer,
    mint,
    owner
  );
  await splToken.mintTo(
    connection,
    payer,
    mint,
    tokenAccount,
    payer,
    amount * Math.pow(10, DECIMALS)
  );
  return tokenAccount;
}

async function main() {
  const clusterUrl = getClusterUrlEnv();
  const makerPubkey = new PublicKey(prompt("Enter maker public key: "));
  const takerPubkey = new PublicKey(prompt("Enter taker public key: "));

  const connection = new Connection(clusterUrl, "confirmed");
  const payerKeypair = await loadKeypairFromFile(PAYER_KEYPAIR_PATH);

  const srcMint = await createMint(connection, payerKeypair);
  console.log(`Created src token mint: ${srcMint.toString()}`);

  const makerSrcAta = await mintTo(
    connection,
    payerKeypair,
    srcMint,
    makerPubkey,
    AMOUNT_TO_MINT
  );
  const makerSrcBalance = (await connection.getTokenAccountBalance(makerSrcAta))
    .value.uiAmount;
  console.log(
    `Created maker src ata with address ${makerSrcAta.toString()} and balance ${makerSrcBalance}`
  );

  const takerSrcAta = await mintTo(
    connection,
    payerKeypair,
    srcMint,
    takerPubkey,
    AMOUNT_TO_MINT
  );
  const takerSrcBalance = (await connection.getTokenAccountBalance(makerSrcAta))
    .value.uiAmount;
  console.log(
    `Created taker src ata with address ${takerSrcAta.toString()} and balance ${takerSrcBalance}`
  );

  const dstMint = await createMint(connection, payerKeypair);
  console.log(`Created dst token mint: ${dstMint.toString()}`);

  const makerDstAta = await mintTo(
    connection,
    payerKeypair,
    dstMint,
    makerPubkey,
    AMOUNT_TO_MINT
  );
  const makerDstBalance = (await connection.getTokenAccountBalance(makerDstAta))
    .value.uiAmount;
  console.log(
    `Created maker dst ata with address ${makerDstAta.toString()} and balance ${makerDstBalance}`
  );

  const takerDstAta = await mintTo(
    connection,
    payerKeypair,
    dstMint,
    takerPubkey,
    AMOUNT_TO_MINT
  );
  const takerDstBalance = (await connection.getTokenAccountBalance(takerDstAta))
    .value.uiAmount;
  console.log(
    `Created taker dst ata with address ${takerDstAta.toString()} and balance ${takerDstBalance}`
  );
}

main();
