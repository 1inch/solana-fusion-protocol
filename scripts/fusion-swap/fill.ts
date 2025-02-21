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
  OrderConfig,
} from "../utils";
import { sha256 } from "@noble/hashes/sha256";

const prompt = require("prompt-sync")({ sigint: true });

async function fill(
  connection: Connection,
  program: Program<FusionSwap>,
  whitelistProgramId: PublicKey,
  takerKeypair: Keypair,
  orderConfig: OrderConfig
): Promise<void> {
  const orderHash = sha256(
    program.coder.types.encode("orderConfig", orderConfig)
  );
  const escrow = findEscrowAddress(
    program.programId,
    orderConfig.maker,
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
      orderConfig,
      new BN(orderConfig.srcAmount * Math.pow(10, srcMintDecimals))
    )
    .accountsPartial({
      taker: takerKeypair.publicKey,
      resolverAccess,
      maker: orderConfig.maker,
      makerReceiver: orderConfig.receiver,
      srcMint: orderConfig.srcMint,
      dstMint: orderConfig.dstMint,
      escrow,
      escrowSrcAta,
      takerSrcAta,
      protocolDstAta: orderConfig.fees.protocolDstAta,
      integratorDstAta: orderConfig.fees.integratorDstAta,
      srcTokenProgram: orderConfig.srcTokenProgram,
      dstTokenProgram: orderConfig.dstTokenProgram,
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
  const orderHash = prompt("Enter order hash: ");
  const connection = new Connection(clusterUrl, "confirmed");
  const fusionSwap = new Program(FUSION_IDL as FusionSwap, { connection });
  const whitelist = new Program(WHITELIST_IDL as Whitelist, { connection });

  try {
    const escrowAddr = findEscrowAddress(
      fusionSwap.programId,
      maker,
      orderHash
    );
    console.log(JSON.stringify(escrowAddr));
  } catch (e) {
    console.error(
      `Escrow with order hash = ${orderHash} and maker = ${maker.toString()} does not exist`
    );
    return;
  }

  const takerKeypairPath = prompt("Enter taker keypair path: ");
  const srcMint = new PublicKey(prompt("Enter src mint public key: "));
  const dstMint = new PublicKey(prompt("Enter dst mint public key: "));
  const amount = Number(prompt("Enter fill amount: "));

  const takerKeypair = await loadKeypairFromFile(takerKeypairPath);

  const orderConfig: OrderConfig = {
    srcMint,
    dstMint,
    makerReceiver: maker,
    srcAmount: amount,
    srcTokenProgram: splToken.TOKEN_PROGRAM_ID,
    dstTokenProgram: splToken.TOKEN_PROGRAM_ID,
  };

  await fill(
    connection,
    fusionSwap,
    whitelist.programId,
    takerKeypair,
    orderConfig
  );
}

main();
