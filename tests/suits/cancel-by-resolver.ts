import * as anchor from "@coral-xyz/anchor";
import * as splToken from "@solana/spl-token";
import { BankrunProvider } from "anchor-bankrun";
import { BanksClient, ProgramTestContext } from "solana-bankrun";
import { FusionSwap } from "../../target/types/fusion_swap";
import chai, { expect } from "chai";
import chaiAsPromised from "chai-as-promised";
import {
  TestState,
  setCurrentTime,
  trackReceivedTokenAndTx,
} from "../utils/utils";

const FusionSwapIDL = require("../../target/idl/fusion_swap.json");
chai.use(chaiAsPromised);

describe("Cancel by Resolver", () => {
  const defaultSrcAmount = new anchor.BN(1000000);
  const defaultCancellationPremium = defaultSrcAmount
    .muln(5 * 100)
    .divn(100 * 100); // 5%
  const defaultMaxCancellationMultiplier = 500; // 50%
  let provider: BankrunProvider;
  let banksClient: BanksClient;
  let context: ProgramTestContext;
  let state: TestState;
  let program: anchor.Program<FusionSwap>;
  let payer: anchor.web3.Keypair;

  const auction = {
    createTime: 0, // We update it before each test
    startTime: 0, // We update it before each test
    duration: 32000,
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
    auction.createTime = Math.floor(new Date().getTime() / 1000);
    auction.startTime = auction.createTime + auction.duration;
    // Rollback clock to the current time after tests that move time forward when order already expired
    await setCurrentTime(context, auction.createTime);
  });

  it("Resolver can cancel the order and receive a portion of the remaining tokens", async () => {
    const cancellationPremiums = [1, 2.5, 7.5].map((percentage) =>
      defaultSrcAmount.muln(percentage * 100).divn(100 * 100)
    );
    for (const minCancellationPremium of cancellationPremiums) {
      await setCurrentTime(context, auction.createTime);
      const escrow = await state.createEscrow({
        escrowProgram: program,
        payer,
        provider: banksClient,
        orderConfig: state.orderConfig({
          srcAmount: defaultSrcAmount,
          expirationTime: auction.createTime + auction.duration,
          fee: {
            minCancellationPremium,
            maxCancellationMultiplier: defaultMaxCancellationMultiplier,
          },
          cancellationAuctionDuration: auction.duration,
        }),
      });

      // Rewind time to expire the order
      await setCurrentTime(context, auction.startTime + 1);

      const transactionPromise = () =>
        program.methods
          .cancelByResolver(escrow.reducedOrderConfig)
          .accountsPartial({
            resolver: state.bob.keypair.publicKey,
            maker: state.alice.keypair.publicKey,
            makerReceiver: escrow.orderConfig.receiver,
            srcMint: escrow.orderConfig.srcMint,
            dstMint: escrow.orderConfig.dstMint,
            escrow: escrow.escrow,
            escrowSrcAta: escrow.ata,
            protocolDstAta: escrow.orderConfig.fee.protocolDstAta,
            integratorDstAta: escrow.orderConfig.fee.integratorDstAta,
            srcTokenProgram: splToken.TOKEN_PROGRAM_ID,
          })
          .signers([state.bob.keypair])
          .rpc();

      const results = await trackReceivedTokenAndTx(
        provider.connection,
        [
          state.alice.atas[state.tokens[0].toString()].address,
          state.bob.atas[state.tokens[0].toString()].address,
        ],
        transactionPromise
      );

      expect(results).to.be.deep.eq([
        BigInt(defaultSrcAmount.sub(minCancellationPremium).toNumber()),
        BigInt(minCancellationPremium.toNumber()),
      ]);
    }
  });

  it("Resolver can cancel the order at different points in the auction time frame", async () => {
    const cancellationPoints = [10, 25, 50, 100].map(
      (percentage) =>
        auction.startTime +
        (auction.duration * (percentage * 100)) / (100 * 100)
    );
    for (const cancellationPoint of cancellationPoints) {
      await setCurrentTime(context, auction.createTime);
      const escrow = await state.createEscrow({
        escrowProgram: program,
        payer,
        provider: banksClient,
        orderConfig: state.orderConfig({
          srcAmount: defaultSrcAmount,
          expirationTime: auction.startTime,
          fee: {
            minCancellationPremium: defaultCancellationPremium,
            maxCancellationMultiplier: defaultMaxCancellationMultiplier,
          },
          cancellationAuctionDuration: auction.duration,
        }),
      });

      await setCurrentTime(context, cancellationPoint);

      const timeElapsed = cancellationPoint - auction.startTime;
      const rateBump = Math.floor(
        (timeElapsed * defaultMaxCancellationMultiplier) / auction.duration
      );
      const resolverPremium = defaultCancellationPremium
        .muln(1e3 + rateBump)
        .divn(1e3);

      const transactionPromise = () =>
        program.methods
          .cancelByResolver(escrow.reducedOrderConfig)
          .accountsPartial({
            resolver: state.bob.keypair.publicKey,
            maker: state.alice.keypair.publicKey,
            makerReceiver: escrow.orderConfig.receiver,
            srcMint: escrow.orderConfig.srcMint,
            dstMint: escrow.orderConfig.dstMint,
            escrow: escrow.escrow,
            escrowSrcAta: escrow.ata,
            protocolDstAta: escrow.orderConfig.fee.protocolDstAta,
            integratorDstAta: escrow.orderConfig.fee.integratorDstAta,
            srcTokenProgram: splToken.TOKEN_PROGRAM_ID,
          })
          .signers([state.bob.keypair])
          .rpc();

      const results = await trackReceivedTokenAndTx(
        provider.connection,
        [
          state.alice.atas[state.tokens[0].toString()].address,
          state.bob.atas[state.tokens[0].toString()].address,
        ],
        transactionPromise
      );

      expect(results).to.be.deep.eq([
        BigInt(defaultSrcAmount.sub(resolverPremium).toNumber()),
        BigInt(resolverPremium.toNumber()),
      ]);
    }
  });

  it("Resolver can cancel the order after auction", async () => {
    const escrow = await state.createEscrow({
      escrowProgram: program,
      payer,
      provider: banksClient,
      orderConfig: state.orderConfig({
        srcAmount: defaultSrcAmount,
        expirationTime: auction.startTime,
        fee: {
          minCancellationPremium: defaultCancellationPremium,
          maxCancellationMultiplier: defaultMaxCancellationMultiplier,
        },
        cancellationAuctionDuration: auction.duration,
      }),
    });

    await setCurrentTime(context, auction.startTime + auction.duration + 1);

    const transactionPromise = () =>
      program.methods
        .cancelByResolver(escrow.reducedOrderConfig)
        .accountsPartial({
          resolver: state.bob.keypair.publicKey,
          maker: state.alice.keypair.publicKey,
          makerReceiver: escrow.orderConfig.receiver,
          srcMint: escrow.orderConfig.srcMint,
          dstMint: escrow.orderConfig.dstMint,
          escrow: escrow.escrow,
          escrowSrcAta: escrow.ata,
          protocolDstAta: escrow.orderConfig.fee.protocolDstAta,
          integratorDstAta: escrow.orderConfig.fee.integratorDstAta,
          srcTokenProgram: splToken.TOKEN_PROGRAM_ID,
        })
        .signers([state.bob.keypair])
        .rpc({ skipPreflight: true });

    const results = await trackReceivedTokenAndTx(
      provider.connection,
      [
        state.alice.atas[state.tokens[0].toString()].address,
        state.bob.atas[state.tokens[0].toString()].address,
      ],
      transactionPromise
    );

    const resolverPremium = defaultCancellationPremium
      .muln(1e3 + defaultMaxCancellationMultiplier)
      .divn(1e3);
    console.log();

    expect(results).to.be.deep.eq([
      BigInt(defaultSrcAmount.sub(resolverPremium).toNumber()),
      BigInt(resolverPremium.toNumber()),
    ]);
  });

  it("Resolver can't cancel if the order has not expired", async () => {
    const escrow = await state.createEscrow({
      escrowProgram: program,
      payer,
      provider: banksClient,
      orderConfig: state.orderConfig({
        fee: {
          minCancellationPremium: defaultCancellationPremium,
        },
        cancellationAuctionDuration: auction.duration,
      }),
    });

    await expect(
      program.methods
        .cancelByResolver(escrow.reducedOrderConfig)
        .accountsPartial({
          resolver: state.bob.keypair.publicKey,
          maker: state.alice.keypair.publicKey,
          makerReceiver: escrow.orderConfig.receiver,
          srcMint: escrow.orderConfig.srcMint,
          dstMint: escrow.orderConfig.dstMint,
          escrow: escrow.escrow,
          escrowSrcAta: escrow.ata,
          protocolDstAta: escrow.orderConfig.fee.protocolDstAta,
          integratorDstAta: escrow.orderConfig.fee.integratorDstAta,
          srcTokenProgram: splToken.TOKEN_PROGRAM_ID,
        })
        .signers([state.bob.keypair])
        .rpc()
    ).to.be.rejectedWith("Error Code: OrderNotExpired");
  });

  it("Resolver can't cancel if the caller is not a whitelisted resolver", async () => {
    const escrow = await state.createEscrow({
      escrowProgram: program,
      payer,
      provider: banksClient,
      orderConfig: state.orderConfig({
        fee: {
          minCancellationPremium: defaultCancellationPremium,
        },
        cancellationAuctionDuration: auction.duration,
      }),
    });

    await setCurrentTime(context, auction.startTime + 1);
    await expect(
      program.methods
        .cancelByResolver(escrow.reducedOrderConfig)
        .accountsPartial({
          resolver: state.charlie.keypair.publicKey,
          maker: state.alice.keypair.publicKey,
          makerReceiver: escrow.orderConfig.receiver,
          srcMint: escrow.orderConfig.srcMint,
          dstMint: escrow.orderConfig.dstMint,
          escrow: escrow.escrow,
          escrowSrcAta: escrow.ata,
          protocolDstAta: escrow.orderConfig.fee.protocolDstAta,
          integratorDstAta: escrow.orderConfig.fee.integratorDstAta,
          srcTokenProgram: splToken.TOKEN_PROGRAM_ID,
        })
        .signers([state.charlie.keypair])
        .rpc()
    ).to.be.rejectedWith(
      "AnchorError caused by account: resolver_access. Error Code: AccountNotInitialized"
    );
  });
});
