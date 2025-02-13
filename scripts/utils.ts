import { Connection, PublicKey, Keypair } from "@solana/web3.js";
import * as anchor from "@coral-xyz/anchor";
import fs from "fs";
import path from "path";
import os from "os";
import * as splToken from "@solana/spl-token";

const FusionSwapIDL = require("../target/idl/fusion_swap.json");

const orderConfigType = FusionSwapIDL.types.find(
  (t) => t.name === "OrderConfig"
);
export type OrderConfig = (typeof orderConfigType)["type"]["fields"];

const feeConfigType = FusionSwapIDL.types.find((t) => t.name === "FeeConfig");
export type FeeConfig = (typeof feeConfigType)["type"]["fields"];

const escrowType = FusionSwapIDL.types.find((t) => t.name === "Escrow");
export type Escrow = (typeof escrowType)["type"]["fields"];

const dutchAuctionDataType = FusionSwapIDL.types.find(
  (t) => t.name === "DutchAuctionData"
);
export type DutchAuctionData = (typeof dutchAuctionDataType)["type"]["fields"];

export const defaultFeeConfig: FeeConfig = {
  protocolFee: 0,
  integratorFee: 0,
  surplusPercentage: 0,
};

export const defaultAuctionData: DutchAuctionData = {
  startTime: 0xffffffff - 32000, // default auction start in the far far future and order use default formula
  duration: 32000,
  initialRateBump: 0,
  pointsAndTimeDeltas: [],
};

export async function getTokenDecimals(
  connection: Connection,
  mint: PublicKey
): Promise<number> {
  const mintAccount = await splToken.getMint(connection, mint);
  return mintAccount.decimals;
}

export async function loadKeypairFromFile(
  filePath: string
): Promise<Keypair | undefined> {
  // This is here so you can also load the default keypair from the file system.
  const resolvedPath = path.resolve(
    filePath.startsWith("~") ? filePath.replace("~", os.homedir()) : filePath
  );

  try {
    const raw = fs.readFileSync(resolvedPath);
    const formattedData = JSON.parse(raw.toString());

    const keypair = Keypair.fromSecretKey(Uint8Array.from(formattedData));
    return keypair;
  } catch (error) {
    throw new Error(
      `Error reading keypair from file: ${(error as Error).message}`
    );
  }
}

export function findEscrowAddress(
  programId: PublicKey,
  maker: PublicKey,
  orderId: number
): PublicKey {
  const [escrow] = PublicKey.findProgramAddressSync(
    [
      anchor.utils.bytes.utf8.encode("escrow"),
      maker.toBuffer(),
      numberToBuffer(orderId, 4),
    ],
    programId
  );

  return escrow;
}

export function findResolverAccessAddress(
  programId: PublicKey,
  user: PublicKey
): PublicKey {
  const [resolverAccess] = PublicKey.findProgramAddressSync(
    [anchor.utils.bytes.utf8.encode("resolver_access"), user.toBuffer()],
    programId
  );

  return resolverAccess;
}

export function findWhitelistStateAddress(programId: PublicKey): PublicKey {
  const [whitelistState] = PublicKey.findProgramAddressSync(
    [anchor.utils.bytes.utf8.encode("whitelist_state")],
    programId
  );

  return whitelistState;
}

export function defaultExpirationTime(): number {
  return ~~(new Date().getTime() / 1000) + 86400; // now + 1 day
}

export function getClusterUrlEnv() {
  const clusterUrl = process.env.CLUSTER_URL;
  if (!clusterUrl) {
    throw new Error("Missing CLUSTER_URL environment variable");
  }
  return clusterUrl;
}

function numberToBuffer(n: number, bufSize: number) {
  return Buffer.from((~~n).toString(16).padStart(bufSize * 2, "0"), "hex");
}
