import * as anchor from "@coral-xyz/anchor";

export type FeeConfig = {
  protocolDstAcc: anchor.web3.PublicKey | null;
  integratorDstAcc: anchor.web3.PublicKey | null;
  protocolFee: number;
  integratorFee: number;
  surplusPercentage: number;
  maxCancellationPremium: anchor.BN;
};

export type PointAndTimeDelta = {
  rateBump: number;
  timeDelta: number;
};

export type AuctionData = {
  startTime: number;
  duration: number;
  initialRateBump: number;
  pointsAndTimeDeltas: Array<PointAndTimeDelta>;
}
//
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
  srcMint: anchor.web3.PublicKey;
  dstMint: anchor.web3.PublicKey;
  receiver: anchor.web3.PublicKey;
};

