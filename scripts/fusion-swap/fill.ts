import {
  Connection,
  Keypair,
  PublicKey,
  Transaction,
  sendAndConfirmTransaction,
} from "@solana/web3.js";
import { BN, Program } from "@coral-xyz/anchor";
import * as splToken from "@solana/spl-token";

import FUSION_IDL from "../../target/idl/fusion_swap.json";
import WHITELIST_IDL from "../../target/idl/whitelist.json";
import { FusionSwap } from "../../target/types/fusion_swap";
import { Whitelist } from "../../target/types/whitelist";
import {
  findEscrowAddress,
  findResolverAccessAddress,
  getClusterUrlEnv,
  getTokenDecimals,
  loadKeypairFromFile,
} from "../utils";

const prompt = require("prompt-sync")({ sigint: true });

async function fill(
  connection: Connection,
  program: Program<FusionSwap>,
  whitelistProgramId: PublicKey,
  takerKeypair: Keypair,
  srcMint: PublicKey,
  dstMint: PublicKey,
  maker: PublicKey,
  orderId: number,
  amount: number,
  makerReceiver: PublicKey = maker,
  protocolDstAta: PublicKey = null,
  integratorDstAta: PublicKey = null,
  srcTokenProgram: PublicKey = splToken.TOKEN_PROGRAM_ID,
  dstTokenProgram: PublicKey = splToken.TOKEN_PROGRAM_ID
): Promise<void> {
  const escrow = findEscrowAddress(program.programId, maker, orderId);
  const escrowSrcAta = await splToken.getAssociatedTokenAddress(
    srcMint,
    escrow,
    true
  );

  const resolverAccess = findResolverAccessAddress(
    whitelistProgramId,
    takerKeypair.publicKey
  );

  const takerSrcAta = await splToken.getAssociatedTokenAddress(
    srcMint,
    takerKeypair.publicKey
  );

  const srcMintDecimals = await getTokenDecimals(connection, srcMint);

  const fillIx = await program.methods
    .fill(orderId, new BN(amount * Math.pow(10, srcMintDecimals)))
    .accountsPartial({
      taker: takerKeypair.publicKey,
      resolverAccess,
      maker,
      makerReceiver,
      srcMint,
      dstMint,
      escrow,
      escrowSrcAta,
      takerSrcAta,
      protocolDstAta,
      integratorDstAta,
      srcTokenProgram,
      dstTokenProgram,
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
  const maker = new PublicKey(prompt("Enter maker public key: "));
  const orderId = Number(prompt("Enter order id: "));

  const connection = new Connection(clusterUrl, "confirmed");
  const fusionSwap = new Program(FUSION_IDL as FusionSwap, { connection });
  const whitelist = new Program(WHITELIST_IDL as Whitelist, { connection });

  try {
    const escrowAddr = findEscrowAddress(fusionSwap.programId, maker, orderId);
    const escrowAccount = await fusionSwap.account.escrow.fetch(escrowAddr);
    console.log(JSON.stringify(escrowAccount));
  } catch (e) {
    console.error(
      `Escrow with order id = ${orderId} and maker = ${maker.toString()} does not exist`
    );
    return;
  }

  const takerKeypairPath = prompt("Enter taker keypair path: ");
  const srcMint = new PublicKey(prompt("Enter src mint public key: "));
  const dstMint = new PublicKey(prompt("Enter dst mint public key: "));
  const amount = Number(prompt("Enter fill amount: "));

  const takerKeypair = await loadKeypairFromFile(takerKeypairPath);

  await fill(
    connection,
    fusionSwap,
    whitelist.programId,
    takerKeypair,
    srcMint,
    dstMint,
    maker,
    orderId,
    amount
  );
}

main();
