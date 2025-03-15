import { Connection, PublicKey, Keypair } from "@solana/web3.js";
import * as anchor from "@coral-xyz/anchor";
import fs from "fs";
import path from "path";
import os from "os";
import * as splToken from "@solana/spl-token";
import { sha256 } from "@noble/hashes/sha256";
import * as borsh from "borsh";

const FusionSwapIDL = require("../target/idl/fusion_swap.json");

const reducedOrderConfigType = FusionSwapIDL.types.find(
  (t) => t.name === "ReducedOrderConfig"
);
export type ReducedOrderConfig =
  (typeof reducedOrderConfigType)["type"]["fields"];

const reducedFeeConfigType = FusionSwapIDL.types.find(
  (t) => t.name === "ReducedFeeConfig"
);
export type ReducedFeeConfig = (typeof reducedFeeConfigType)["type"]["fields"];

export type FeeConfig = {
  protocolDstAcc: anchor.web3.PublicKey | null;
  integratorDstAcc: anchor.web3.PublicKey | null;
  protocolFee: number;
  integratorFee: number;
  surplusPercentage: number;
  maxCancellationPremium: anchor.BN;
};
export type OrderConfig = ReducedOrderConfig & {
  srcMint: anchor.web3.PublicKey;
  dstMint: anchor.web3.PublicKey;
  receiver: anchor.web3.PublicKey;
  fee: FeeConfig;
  cancellationAuctionDuration: number;
};

const escrowType = FusionSwapIDL.types.find((t) => t.name === "Escrow");
export type Escrow = (typeof escrowType)["type"]["fields"];

const auctionDataType = FusionSwapIDL.types.find(
  (t) => t.name === "AuctionData"
);
export type AuctionData = (typeof auctionDataType)["type"]["fields"];

export const defaultFeeConfig: ReducedFeeConfig = {
  protocolFee: 0,
  integratorFee: 0,
  surplusPercentage: 0,
  maxCancellationPremium: new anchor.BN(0),
};

export const defaultAuctionData: AuctionData = {
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
  orderHash: Buffer | string
): PublicKey {
  if (typeof orderHash === "string") {
    const arr = Array.from(orderHash.match(/../g) || [], (h) =>
      parseInt(h, 16)
    );
    orderHash = Buffer.from(arr);
  }

  const [escrow] = PublicKey.findProgramAddressSync(
    [
      anchor.utils.bytes.utf8.encode("escrow"),
      maker.toBuffer(),
      Buffer.from(orderHash),
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

export function calculateOrderHash(orderConfig: OrderConfig): Uint8Array {
  const values = {
    id: orderConfig.id,
    srcAmount: orderConfig.srcAmount.toNumber(),
    minDstAmount: orderConfig.minDstAmount.toNumber(),
    estimatedDstAmount: orderConfig.estimatedDstAmount.toNumber(),
    expirationTime: orderConfig.expirationTime,
    nativeDstAsset: orderConfig.nativeDstAsset,
    fee: {
      protocolFee: orderConfig.fee.protocolFee,
      integratorFee: orderConfig.fee.integratorFee,
      surplusPercentage: orderConfig.fee.surplusPercentage,
      maxCancellationPremium: orderConfig.fee.maxCancellationPremium,
    },
    dutchAuctionData: {
      startTime: orderConfig.dutchAuctionData.startTime,
      duration: orderConfig.dutchAuctionData.duration,
      initialRateBump: orderConfig.dutchAuctionData.initialRateBump,
      pointsAndTimeDeltas: orderConfig.dutchAuctionData.pointsAndTimeDeltas.map(
        (p) => ({
          rateBump: p.rateBump,
          timeDelta: p.timeDelta,
        })
      ),
    },
    cancellationAuctionDuration: orderConfig.cancellationAuctionDuration,

    // Accounts concatenated directly to OrderConfig
    protocolDstAcc: orderConfig.fee.protocolDstAcc?.toBuffer(),
    integratorDstAcc: orderConfig.fee.integratorDstAcc?.toBuffer(),
    srcMint: orderConfig.srcMint.toBuffer(),
    dstMint: orderConfig.dstMint.toBuffer(),
    receiver: orderConfig.receiver.toBuffer(),
  };

  return sha256(borsh.serialize(orderConfigSchema, values));
}

const orderConfigSchema = {
  struct: {
    id: "u32",
    srcAmount: "u64",
    minDstAmount: "u64",
    estimatedDstAmount: "u64",
    expirationTime: "u32",
    nativeDstAsset: "bool",
    fee: {
      struct: {
        protocolFee: "u16",
        integratorFee: "u16",
        surplusPercentage: "u8",
        maxCancellationPremium: "u64",
      },
    },
    dutchAuctionData: {
      struct: {
        startTime: "u32",
        duration: "u32",
        initialRateBump: "u16",
        pointsAndTimeDeltas: {
          array: {
            type: {
              struct: {
                rateBump: "u16",
                timeDelta: "u16",
              },
            },
          },
        },
      },
    },
    cancellationAuctionDuration: "u32",

    // Accounts concatenated directly to OrderConfig
    protocolDstAcc: { option: { array: { type: "u8", len: 32 } } },
    integratorDstAcc: { option: { array: { type: "u8", len: 32 } } },
    srcMint: { array: { type: "u8", len: 32 } },
    dstMint: { array: { type: "u8", len: 32 } },
    receiver: { array: { type: "u8", len: 32 } },
  },
};
