import * as anchor from "@coral-xyz/anchor";
import * as splBankrunToken from "spl-token-bankrun";
import * as splToken from "@solana/spl-token";
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
import { calculateOrderHash } from "../../scripts/utils";
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

  const auction = {
    startTime: 0, // we update it before each test
    duration: 32000,
    initialRateBump: 50000,
    pointsAndTimeDeltas: [
      { rateBump: 20000, timeDelta: 10000 },
      { rateBump: 10000, timeDelta: 20000 },
    ],
  };

  before(async () => {
    const usersKeypairs = [];
    for (let i = 0; i < 4; i++) {
      usersKeypairs.push(anchor.web3.Keypair.generate());
    }
    context = await TestState.bankrunContext(usersKeypairs);
    provider = new BankrunProvider(context);
    anchor.setProvider(provider);
    banksClient = context.banksClient;
    payer = context.payer;

    program = new anchor.Program<FusionSwap>(FusionSwapIDL, provider);

    state = await TestState.bankrunCreate(context, payer, usersKeypairs, {
      tokensNums: 3,
    });
  });

  beforeEach(async () => {
    auction.startTime = Math.floor(new Date().getTime() / 1000);

    // Rollback clock to the current time after tests that move time forward when order already expired
    await setCurrentTime(context, auction.startTime);

    state.escrows[0] = await state.createEscrow({
      escrowProgram: program,
      payer,
      provider: banksClient,
      orderConfig: {
        dutchAuctionData: auction,
      },
    });
  });

  it("should not work after the expiration time", async () => {
    await setCurrentTime(context, state.defaultExpirationTime);
    await expect(
      program.methods
        .fill(state.escrows[0].reducedOrderConfig, state.defaultSrcAmount)
        .accounts(state.buildAccountsDataForFill({}))
        .signers([state.bob.keypair])
        .rpc()
    ).to.be.rejectedWith("Error Code: OrderExpired");
  });

  // This test has nothing to do with the dutch auction logic and placed
  // here only because this test suite uses bankrun.
  it("should not create the escrow after the expiration time", async () => {
    await setCurrentTime(context, state.defaultExpirationTime);

    const orderConfig = state.orderConfig({});

    const [escrow] = anchor.web3.PublicKey.findProgramAddressSync(
      [
        anchor.utils.bytes.utf8.encode("escrow"),
        state.alice.keypair.publicKey.toBuffer(),
        calculateOrderHash(orderConfig),
      ],
      program.programId
    );

    await expect(
      program.methods
        .create(orderConfig)
        .accountsPartial({
          maker: state.alice.keypair.publicKey,
          makerReceiver: orderConfig.receiver,
          srcMint: state.tokens[0],
          dstMint: state.tokens[1],
          protocolDstAcc: null,
          integratorDstAcc: null,
          escrow: escrow,
          srcTokenProgram: splToken.TOKEN_PROGRAM_ID,
        })
        .signers([state.alice.keypair])
        .rpc()
    ).to.be.rejectedWith("Error Code: OrderExpired");
  });

  it("should fill with initialRateBump before auction started", async () => {
    await setCurrentTime(context, auction.startTime - 1000);

    const transactionPromise = () =>
      program.methods
        .fill(state.escrows[0].reducedOrderConfig, state.defaultSrcAmount)
        .accountsPartial(state.buildAccountsDataForFill({}))
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
      auction.startTime + auction.pointsAndTimeDeltas[0].timeDelta / 2
    );

    const transactionPromise = () =>
      program.methods
        .fill(state.escrows[0].reducedOrderConfig, state.defaultSrcAmount)
        .accountsPartial(state.buildAccountsDataForFill({}))
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

    const dstAmountWithRateBumpMax = BigInt(
      (state.defaultDstAmount.toNumber() *
        (BASE_POINTS + auction.initialRateBump)) /
        BASE_POINTS
    );
    const dstAmountWithRateBumpMin = BigInt(
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
      auction.startTime +
        auction.pointsAndTimeDeltas[0].timeDelta +
        auction.pointsAndTimeDeltas[1].timeDelta / 2
    );

    const transactionPromise = () =>
      program.methods
        .fill(state.escrows[0].reducedOrderConfig, state.defaultSrcAmount)
        .accountsPartial(state.buildAccountsDataForFill({}))
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

    const dstAmountWithRateBumpMax = BigInt(
      (state.defaultDstAmount.toNumber() *
        (BASE_POINTS + auction.initialRateBump)) /
        BASE_POINTS
    );
    const dstAmountWithRateBumpMin = BigInt(
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
    await setCurrentTime(context, auction.startTime + auction.duration + 1);

    const transactionPromise = () =>
      program.methods
        .fill(state.escrows[0].reducedOrderConfig, state.defaultSrcAmount)
        .accountsPartial(state.buildAccountsDataForFill({}))
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

  it("Execute the trade with surplus", async () => {
    state.escrows[0] = await state.createEscrow({
      escrowProgram: program,
      payer,
      provider: banksClient,
      orderConfig: {
        fee: {
          protocolDstAcc:
            state.charlie.atas[state.tokens[1].toString()].address,
          surplusPercentage: 50, // 50%
          integratorDstAcc: undefined,
          protocolFee: undefined,
          integratorFee: undefined,
          maxCancellationPremium: undefined
        },
        dutchAuctionData: auction,
      },
    });

    const transactionPromise = () =>
      program.methods
        .fill(state.escrows[0].reducedOrderConfig, state.defaultSrcAmount)
        .accountsPartial(
          state.buildAccountsDataForFill({
            escrow: state.escrows[0].escrow,
            escrowSrcAta: state.escrows[0].ata,
            protocolDstAcc:
              state.charlie.atas[state.tokens[1].toString()].address,
          })
        )
        .signers([state.bob.keypair])
        .rpc();

    const results = await trackReceivedTokenAndTx(
      provider.connection,
      [
        state.alice.atas[state.tokens[1].toString()].address,
        state.bob.atas[state.tokens[0].toString()].address,
        state.bob.atas[state.tokens[1].toString()].address,
        state.charlie.atas[state.tokens[1].toString()].address,
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
    const surplus =
      (dstAmountWithRateBump - BigInt(state.defaultDstAmount.toNumber())) / 2n;
    expect(results).to.be.deep.eq([
      dstAmountWithRateBump - surplus,
      BigInt(state.defaultSrcAmount.toNumber()),
      -dstAmountWithRateBump,
      surplus,
    ]);
  });

  it("Execute the trade with all fees", async () => {
    const auction = {
      startTime: Math.floor(new Date().getTime() / 1000),
      duration: 32000,
      initialRateBump: 50000,
      pointsAndTimeDeltas: [],
    };

    state.escrows[0] = await state.createEscrow({
      escrowProgram: program,
      payer,
      provider: banksClient,
      orderConfig: {
        fee: {
          protocolDstAcc:
            state.charlie.atas[state.tokens[1].toString()].address,
          integratorDstAcc: state.dave.atas[state.tokens[1].toString()].address,
          protocolFee: 10000, // 10%
          integratorFee: 15000, // 15%
          surplusPercentage: 50, // 50%
          maxCancellationPremium: undefined
        },
        dutchAuctionData: auction,
      },
    });

    const transactionPromise = () =>
      program.methods
        .fill(state.escrows[0].reducedOrderConfig, state.defaultSrcAmount)
        .accountsPartial(
          state.buildAccountsDataForFill({
            escrow: state.escrows[0].escrow,
            escrowSrcAta: state.escrows[0].ata,
            protocolDstAcc:
              state.charlie.atas[state.tokens[1].toString()].address,
            integratorDstAcc:
              state.dave.atas[state.tokens[1].toString()].address,
          })
        )
        .signers([state.bob.keypair])
        .rpc();

    const results = await trackReceivedTokenAndTx(
      provider.connection,
      [
        state.alice.atas[state.tokens[1].toString()].address,
        state.bob.atas[state.tokens[0].toString()].address,
        state.bob.atas[state.tokens[1].toString()].address,
        state.charlie.atas[state.tokens[1].toString()].address,
        state.dave.atas[state.tokens[1].toString()].address,
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
    const integratorFee = (dstAmountWithRateBump * 15n) / 100n;
    const protocolFee = dstAmountWithRateBump / 10n;
    const surplus =
      (dstAmountWithRateBump -
        integratorFee -
        protocolFee -
        BigInt(state.defaultDstAmount.toNumber())) /
      2n;

    expect(results).to.be.deep.eq([
      dstAmountWithRateBump - integratorFee - protocolFee - surplus,
      BigInt(state.defaultSrcAmount.toNumber()),
      -dstAmountWithRateBump,
      protocolFee + surplus, // 10% of takingAmount + 50% *  (actualAmount - estimatedAmpount)
      integratorFee,
    ]);
  });
});
