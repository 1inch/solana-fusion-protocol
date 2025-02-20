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
  const defaultCancellationFee = 5000; // 5%
  const defaultSrcAmount = new anchor.BN(1000000);
  let provider: BankrunProvider;
  let banksClient: BanksClient;
  let context: ProgramTestContext;
  let state: TestState;
  let program: anchor.Program<FusionSwap>;
  let payer: anchor.web3.Keypair;
  let expirationTime;

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
    const currentTime = Math.floor(Date.now() / 1000);
    expirationTime = currentTime + 60; // 1 minute expiration
    await setCurrentTime(context, currentTime);
  });

  it("Resolver can cancel the order and receive a portion of the remaining tokens", async () => {
    const cancellationFees = [1000, 2500, 7500]; // 1%, 2.5%, 7.5%
    for (const cancellationFee of cancellationFees) {
      await setCurrentTime(context, Math.floor(Date.now() / 1000));
      const escrow = await state.createEscrow({
        escrowProgram: program,
        payer,
        provider: banksClient,
        orderConfig: state.orderConfig({
          srcAmount: defaultSrcAmount,
          fee: {
            cancellationFee,
          },
          expirationTime,
        }),
      });

      // Rewind time to expire the order
      await setCurrentTime(context, expirationTime + 1);

      const transactionPromise = () =>
        program.methods
          .cancelByResolver(escrow.order_id)
          .accountsPartial({
            resolver: state.bob.keypair.publicKey,
            maker: state.alice.keypair.publicKey,
            srcMint: state.tokens[0],
            escrow: escrow.escrow,
            escrowSrcAta: escrow.ata,
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
        BigInt(
          Math.floor(
            (defaultSrcAmount.toNumber() * (1e5 - cancellationFee)) / 1e5
          )
        ),
        BigInt(
          Math.floor((defaultSrcAmount.toNumber() * cancellationFee) / 1e5)
        ),
      ]);
    }
  });

  it("Resolver can't cancel if the order has not expired", async () => {
    const escrow = await state.createEscrow({
      escrowProgram: program,
      payer,
      provider: banksClient,
      orderConfig: state.orderConfig({
        fee: {
          cancellationFee: defaultCancellationFee,
        },
        expirationTime,
      }),
    });

    await expect(
      program.methods
        .cancelByResolver(escrow.order_id)
        .accountsPartial({
          resolver: state.bob.keypair.publicKey,
          maker: state.alice.keypair.publicKey,
          srcMint: state.tokens[0],
          escrow: escrow.escrow,
          escrowSrcAta: escrow.ata,
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
          cancellationFee: defaultCancellationFee,
        },
        expirationTime,
      }),
    });

    await setCurrentTime(context, expirationTime + 1);

    await expect(
      program.methods
        .cancelByResolver(escrow.order_id)
        .accountsPartial({
          resolver: state.charlie.keypair.publicKey,
          maker: state.alice.keypair.publicKey,
          srcMint: state.tokens[0],
          escrow: escrow.escrow,
          escrowSrcAta: escrow.ata,
          srcTokenProgram: splToken.TOKEN_PROGRAM_ID,
        })
        .signers([state.charlie.keypair])
        .rpc()
    ).to.be.rejectedWith(
      "AnchorError caused by account: resolver_access. Error Code: AccountNotInitialized"
    );
  });
});
