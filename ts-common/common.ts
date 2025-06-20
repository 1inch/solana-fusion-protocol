import * as anchor from "@coral-xyz/anchor";

const FusionSwapIDL = require("../target/idl/fusion_swap.json");

const escrowType = FusionSwapIDL.types.find((t) => t.name === "Escrow");
export type Escrow = (typeof escrowType)["type"]["fields"];

const auctionDataType = FusionSwapIDL.types.find(
  (t) => t.name === "AuctionData"
);
export type AuctionData = (typeof auctionDataType)["type"]["fields"];

export type FeeConfig = {
  protocolDstAcc: anchor.web3.PublicKey | null;
  integratorDstAcc: anchor.web3.PublicKey | null;
  protocolFee: number;
  integratorFee: number;
  surplusPercentage: number;
  maxCancellationPremium: anchor.BN;
};

export type OrderConfig = {
  id : number;
  srcAmount: anchor.BN;
  minDstAmount: anchor.BN;
  estimatedDstAmount: anchor.BN;
  expirationTime: number;
  srcAssetIsNative: boolean;
  dstAssetIsNative: boolean;
  fee: FeeConfig;
  dutchAuctionData: AuctionData;
  cancellationAuctionDuration: number;
  srcMint: anchor.web3.PublicKey | null;
  dstMint: anchor.web3.PublicKey | null;
  receiver: anchor.web3.PublicKey | null;
};
