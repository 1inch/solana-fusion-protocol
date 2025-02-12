import * as anchor from "@coral-xyz/anchor";
import { FusionSwap } from "../../target/types/fusion_swap";
import chai, { expect } from "chai";
import chaiAsPromised from "chai-as-promised";
import {
  TestState,
  trackReceivedTokenAndTx,
  debugLog,
  numberToBuffer,
} from "../utils/utils";
import { BankrunProvider } from "anchor-bankrun";
import { BanksClient, ProgramTestContext } from "solana-bankrun";
chai.use(chaiAsPromised);

describe.only("Dutch Auction", () => {
  let payer: anchor.web3.Keypair;
  let provider: BankrunProvider;
  let banksClient: BanksClient;
  let context: ProgramTestContext;
  let state: TestState;

  const program = anchor.workspace.FusionSwap as anchor.Program<FusionSwap>;

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

    state = await TestState.bankrunCreate(banksClient, payer, usersKeypairs, {
      tokensNums: 3,
    });
  });

  it("Should work without auction", async () => {
    state.escrows[0] = await state.initEscrow({
      escrowProgram: program,
      payer,
      provider: banksClient,
      // dutch_auction_data: null,
    });

    // const transactionPromise = () =>
    //   program.methods
    //     .fill(escrow.order_id, state.defaultSrcAmount)
    //     .accounts(state.buildAccountsDataForFill({}))
    //     .signers([state.bob.keypair])
    //     .rpc();

    // const results = await trackReceivedTokenAndTx(
    //   provider.connection,
    //   [
    //     state.alice.atas[state.tokens[1].toString()].address,
    //     state.bob.atas[state.tokens[0].toString()].address,
    //     state.bob.atas[state.tokens[1].toString()].address,
    //   ],
    //   transactionPromise
    // );
    // await expect(
    //   splToken.getAccount(provider.connection, escrow.ata)
    // ).to.be.rejectedWith(splToken.TokenAccountNotFoundError);

    // expect(results).to.be.deep.eq([
    //   BigInt(state.defaultDstAmount.toNumber()),
    //   BigInt(state.defaultSrcAmount.toNumber()),
    //   -BigInt(state.defaultDstAmount.toNumber()),
    // ]);
  });
});
