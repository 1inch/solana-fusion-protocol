import * as anchor from "@coral-xyz/anchor";
import * as splBankrunToken from "spl-token-bankrun";
import { FusionSwap } from "../../target/types/fusion_swap";
import chai, { expect } from "chai";
import chaiAsPromised from "chai-as-promised";
import {
  setCurrentTime,
  TestState,
  trackReceivedTokenAndTx,
} from "../utils/utils";
import { BankrunProvider } from "anchor-bankrun";
import { BanksClient, ProgramTestContext } from "solana-bankrun";
chai.use(chaiAsPromised);

const FusionSwapIDL = require("../../target/idl/fusion_swap.json");
const BASE_POINTS = 100000;

function arraysBetweenEqual(actual: BigInt[], min: BigInt[], max: BigInt[]) {
  expect(actual.length).to.equal(min.length);
  expect(actual.length).to.equal(max.length);
  for (let i = 0; i < actual.length; i++) {
    expect(actual[i] >= min[i]).to.be.true;
    expect(actual[i] <= max[i]).to.be.true;
  }
}

describe("Dutch Auction", () => {
  let payer: anchor.web3.Keypair;
  let provider: BankrunProvider;
  let banksClient: BanksClient;
  let context: ProgramTestContext;
  let state: TestState;
  let program: anchor.Program<FusionSwap>;

  before(async () => {
    const usersKeypairs = [];
    for (let i = 0; i < 3; i++) {
      usersKeypairs.push(anchor.web3.Keypair.generate());
    }
    context = await TestState.bankrunContext(usersKeypairs);
    provider = new BankrunProvider(context);
    anchor.setProvider(provider);
    banksClient = context.banksClient;
    payer = context.payer;

    program = new anchor.Program<FusionSwap>(FusionSwapIDL, provider);

    state = await TestState.bankrunCreate(banksClient, payer, usersKeypairs, {
      tokensNums: 3,
    });
  });

  it("should work without auction", async () => {
    state.escrows[0] = await state.initEscrow({
      escrowProgram: program,
      payer,
      provider: banksClient,
      dutch_auction_data: null,
    });

    const transactionPromise = () =>
      program.methods
        .fill(state.escrows[0].order_id, state.defaultSrcAmount)
        .accounts(state.buildAccountsDataForFill({}))
        .signers([state.bob.keypair])
        .rpc();

    const results = await trackReceivedTokenAndTx(
      provider.connection,
      [
        state.alice.atas[state.tokens[1].toString()].address,
        state.bob.atas[state.tokens[0].toString()].address,
        state.bob.atas[state.tokens[1].toString()].address,
      ],
      transactionPromise
    );
    await expect(
      splBankrunToken.getAccount(provider.connection, state.escrows[0].ata)
    ).to.be.rejectedWith(splBankrunToken.TokenAccountNotFoundError);

    expect(results).to.be.deep.eq([
      BigInt(state.defaultDstAmount.toNumber()),
      BigInt(state.defaultSrcAmount.toNumber()),
      -BigInt(state.defaultDstAmount.toNumber()),
    ]);
  });

  describe("Auction", () => {
    const auction = {
      auctionStartTime: 0, // we update it before each test
      get auctionFinishTime() {
        return this.auctionStartTime + 32000;
      },
      initialRateBump: 10000,
      pointsAndTimeDeltas: [
        { rateBump: 20000, timeDelta: 10000 },
        { rateBump: 50000, timeDelta: 20000 },
      ],
    };

    beforeEach(async () => {
      auction.auctionStartTime = Math.floor(new Date().getTime() / 1000);

      // rollback clock to the current time after tests that move time forward when order already expired
      await setCurrentTime(context, auction.auctionStartTime);

      state.escrows[0] = await state.initEscrow({
        escrowProgram: program,
        payer,
        provider: banksClient,
        dutchAuctionData: auction,
      });
    });

    it("should not work after the expiration time", async () => {
      await setCurrentTime(context, state.defaultExpirationTime + 1);
      await expect(
        program.methods
          .fill(state.escrows[0].order_id, state.defaultSrcAmount)
          .accounts(state.buildAccountsDataForFill({}))
          .signers([state.bob.keypair])
          .rpc()
      ).to.be.rejectedWith("Error Code: OrderExpired");
    });

    it("should fill with initialRateBump before auction started", async () => {
      await setCurrentTime(context, auction.auctionStartTime - 1000);

      const transactionPromise = () =>
        program.methods
          .fill(state.escrows[0].order_id, state.defaultSrcAmount)
          .accounts(state.buildAccountsDataForFill({}))
          .signers([state.bob.keypair])
          .rpc();

      const results = await trackReceivedTokenAndTx(
        provider.connection,
        [
          state.alice.atas[state.tokens[1].toString()].address,
          state.bob.atas[state.tokens[0].toString()].address,
          state.bob.atas[state.tokens[1].toString()].address,
        ],
        transactionPromise
      );
      await expect(
        splBankrunToken.getAccount(provider.connection, state.escrows[0].ata)
      ).to.be.rejectedWith(splBankrunToken.TokenAccountNotFoundError);

      const dstAmountWithRateBump = BigInt(
        (state.defaultDstAmount.toNumber() *
          (BASE_POINTS + auction.initialRateBump)) /
          BASE_POINTS
      );
      expect(results).to.be.deep.eq([
        dstAmountWithRateBump,
        BigInt(state.defaultSrcAmount.toNumber()),
        -dstAmountWithRateBump,
      ]);
    });

    it("should fill with another price after auction started, but before first point", async () => {
      await setCurrentTime(
        context,
        auction.auctionStartTime + auction.pointsAndTimeDeltas[0].timeDelta / 2
      );

      const transactionPromise = () =>
        program.methods
          .fill(state.escrows[0].order_id, state.defaultSrcAmount)
          .accounts(state.buildAccountsDataForFill({}))
          .signers([state.bob.keypair])
          .rpc();

      const results = await trackReceivedTokenAndTx(
        provider.connection,
        [
          state.alice.atas[state.tokens[1].toString()].address,
          state.bob.atas[state.tokens[0].toString()].address,
          state.bob.atas[state.tokens[1].toString()].address,
        ],
        transactionPromise
      );
      await expect(
        splBankrunToken.getAccount(provider.connection, state.escrows[0].ata)
      ).to.be.rejectedWith(splBankrunToken.TokenAccountNotFoundError);

      const dstAmountWithRateBumpMin = BigInt(
        (state.defaultDstAmount.toNumber() *
          (BASE_POINTS + auction.initialRateBump)) /
          BASE_POINTS
      );
      const dstAmountWithRateBumpMax = BigInt(
        (state.defaultDstAmount.toNumber() *
          (BASE_POINTS + auction.pointsAndTimeDeltas[0].rateBump)) /
          BASE_POINTS
      );
      arraysBetweenEqual(
        results,
        [
          dstAmountWithRateBumpMin,
          BigInt(state.defaultSrcAmount.toNumber()),
          -dstAmountWithRateBumpMax,
        ],
        [
          dstAmountWithRateBumpMax,
          BigInt(state.defaultSrcAmount.toNumber()),
          -dstAmountWithRateBumpMin,
        ]
      );
    });

    it("should fill with another price after between points", async () => {
      await setCurrentTime(
        context,
        auction.auctionStartTime +
          auction.pointsAndTimeDeltas[0].timeDelta +
          auction.pointsAndTimeDeltas[1].timeDelta / 2
      );

      const transactionPromise = () =>
        program.methods
          .fill(state.escrows[0].order_id, state.defaultSrcAmount)
          .accounts(state.buildAccountsDataForFill({}))
          .signers([state.bob.keypair])
          .rpc();

      const results = await trackReceivedTokenAndTx(
        provider.connection,
        [
          state.alice.atas[state.tokens[1].toString()].address,
          state.bob.atas[state.tokens[0].toString()].address,
          state.bob.atas[state.tokens[1].toString()].address,
        ],
        transactionPromise
      );
      await expect(
        splBankrunToken.getAccount(provider.connection, state.escrows[0].ata)
      ).to.be.rejectedWith(splBankrunToken.TokenAccountNotFoundError);

      const dstAmountWithRateBumpMin = BigInt(
        (state.defaultDstAmount.toNumber() *
          (BASE_POINTS + auction.initialRateBump)) /
          BASE_POINTS
      );
      const dstAmountWithRateBumpMax = BigInt(
        (state.defaultDstAmount.toNumber() *
          (BASE_POINTS + auction.pointsAndTimeDeltas[1].rateBump)) /
          BASE_POINTS
      );
      arraysBetweenEqual(
        results,
        [
          dstAmountWithRateBumpMin,
          BigInt(state.defaultSrcAmount.toNumber()),
          -dstAmountWithRateBumpMax,
        ],
        [
          dstAmountWithRateBumpMax,
          BigInt(state.defaultSrcAmount.toNumber()),
          -dstAmountWithRateBumpMin,
        ]
      );
    });

    it("should fill with default price after auction finished", async () => {
      await setCurrentTime(context, auction.auctionFinishTime + 1);

      const transactionPromise = () =>
        program.methods
          .fill(state.escrows[0].order_id, state.defaultSrcAmount)
          .accounts(state.buildAccountsDataForFill({}))
          .signers([state.bob.keypair])
          .rpc();

      const results = await trackReceivedTokenAndTx(
        provider.connection,
        [
          state.alice.atas[state.tokens[1].toString()].address,
          state.bob.atas[state.tokens[0].toString()].address,
          state.bob.atas[state.tokens[1].toString()].address,
        ],
        transactionPromise
      );
      await expect(
        splBankrunToken.getAccount(provider.connection, state.escrows[0].ata)
      ).to.be.rejectedWith(splBankrunToken.TokenAccountNotFoundError);

      expect(results).to.be.deep.eq([
        BigInt(state.defaultDstAmount.toNumber()),
        BigInt(state.defaultSrcAmount.toNumber()),
        -BigInt(state.defaultDstAmount.toNumber()),
      ]);
    });
  });
});
