import * as anchor from "@coral-xyz/anchor";
import * as splToken from "@solana/spl-token";
import { FusionSwap } from "../../target/types/fusion_swap";
import NodeWallet from "@coral-xyz/anchor/dist/cjs/nodewallet";
import chai, { expect } from "chai";
import chaiAsPromised from "chai-as-promised";
import {
  TestState,
  buildEscrowTraits,
  trackReceivedTokenAndTx,
  debugLog,
  numberToBuffer,
} from "../utils/utils";
chai.use(chaiAsPromised);

async function assertThrowsAsync(action, msg) {
  try {
    await action();
  } catch (e) {
    expect(e.toString().indexOf(msg)).to.be.not.eq(-1);
    return;
  }
  throw new Error("Should have thrown an error but didn't");
}

describe("Fusion Swap", () => {
  const provider = anchor.AnchorProvider.env();
  anchor.setProvider(provider);

  const program = anchor.workspace.FusionSwap as anchor.Program<FusionSwap>;

  const payer = (provider.wallet as NodeWallet).payer;
  debugLog(`Payer ::`, payer.publicKey.toString());

  let state: TestState;

  before(async () => {
    state = await TestState.anchorCreate(provider, payer, { tokensNums: 3 });
  });

  beforeEach(async () => {
    state.escrows = [];
    for (let i = 0; i < 2; ++i) {
      const escrow = await state.initEscrow({
        escrowProgram: program,
        payer,
        provider,
      });
      state.escrows.push(escrow);
      debugLog(`Escrow_${escrow.order_id} ::`, escrow.escrow.toString());
      debugLog(`escrowAta_${escrow.order_id} ::`, escrow.ata.toString());
    }
  });

  describe("Single escrow", () => {
    it("Execute the trade", async () => {
      const transactionPromise = () =>
        program.methods
          .fill(state.escrows[0].order_id, state.defaultSrcAmount)
          .accounts(state.buildAccountsDataForAccept({}))
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
        splToken.getAccount(provider.connection, state.escrows[0].ata)
      ).to.be.rejectedWith(splToken.TokenAccountNotFoundError);

      expect(results).to.be.deep.eq([
        BigInt(state.defaultDstAmount.toNumber()),
        BigInt(state.defaultSrcAmount.toNumber()),
        -BigInt(state.defaultDstAmount.toNumber()),
      ]);
    });

    it("Execute the trade with different maker's receiver", async () => {
      const escrow = await state.initEscrow({
        escrowProgram: program,
        payer,
        provider,
        makerReceiver: state.charlie.keypair.publicKey,
      });
      const transactionPromise = () =>
        program.methods
          .fill(escrow.order_id, state.defaultSrcAmount)
          .accounts(
            state.buildAccountsDataForAccept({
              makerReceiver: state.charlie.keypair.publicKey,
              escrow: escrow.escrow,
              escrowSrcAta: escrow.ata,
              makerDstAta:
                state.charlie.atas[state.tokens[1].toString()].address,
            })
          )
          .signers([state.bob.keypair])
          .rpc();

      const results = await trackReceivedTokenAndTx(
        provider.connection,
        [
          state.charlie.atas[state.tokens[1].toString()].address,
          state.bob.atas[state.tokens[0].toString()].address,
          state.bob.atas[state.tokens[1].toString()].address,
        ],
        transactionPromise
      );
      await expect(
        splToken.getAccount(provider.connection, escrow.ata)
      ).to.be.rejectedWith(splToken.TokenAccountNotFoundError);

      expect(results).to.be.deep.eq([
        BigInt(state.defaultDstAmount.toNumber()),
        BigInt(state.defaultSrcAmount.toNumber()),
        -BigInt(state.defaultDstAmount.toNumber()),
      ]);
    });

    it("Execute the trade with different taker's receiver wallet", async () => {
      const transactionPromise = () =>
        program.methods
          .fill(state.escrows[0].order_id, state.defaultSrcAmount)
          .accounts(
            state.buildAccountsDataForAccept({
              takerSrcAta:
                state.charlie.atas[state.tokens[0].toString()].address,
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
          state.charlie.atas[state.tokens[0].toString()].address,
          state.charlie.atas[state.tokens[1].toString()].address,
        ],
        transactionPromise
      );
      await expect(
        splToken.getAccount(provider.connection, state.escrows[0].ata)
      ).to.be.rejectedWith(splToken.TokenAccountNotFoundError);

      expect(results).to.be.deep.eq([
        BigInt(state.defaultDstAmount.toNumber()),
        BigInt(0),
        -BigInt(state.defaultDstAmount.toNumber()),
        BigInt(state.defaultSrcAmount.toNumber()),
        BigInt(0),
      ]);
    });

    it("Doesn't execute the trade when maker's token account belongs to wrong mint", async () => {
      await expect(
        program.methods
          .fill(state.escrows[0].order_id, state.defaultSrcAmount)
          .accounts(
            state.buildAccountsDataForAccept({
              takerDstAta: state.bob.atas[state.tokens[2].toString()].address,
            })
          )
          .signers([state.bob.keypair])
          .rpc()
      ).to.be.rejectedWith("Error Code: ConstraintAssociated");
    });

    it("Execute the trade with native tokens => tokens", async () => {
      const escrow = await state.initEscrow({
        escrowProgram: program,
        payer,
        provider,
        srcMint: splToken.NATIVE_MINT,
      });

      const transactionPromise = () =>
        program.methods
          .fill(escrow.order_id, state.defaultSrcAmount)
          .accounts(
            state.buildAccountsDataForAccept({
              srcMint: splToken.NATIVE_MINT,
              escrow: escrow.escrow,
              escrowSrcAta: escrow.ata,
              makerSrcAta:
                state.alice.atas[splToken.NATIVE_MINT.toString()].address,
              takerSrcAta:
                state.bob.atas[splToken.NATIVE_MINT.toString()].address,
            })
          )
          .signers([state.bob.keypair])
          .rpc();

      const results = await trackReceivedTokenAndTx(
        provider.connection,
        [
          state.alice.atas[state.tokens[1].toString()].address,
          state.bob.atas[splToken.NATIVE_MINT.toString()].address,
          state.bob.atas[state.tokens[1].toString()].address,
        ],
        transactionPromise
      );
      await expect(
        splToken.getAccount(provider.connection, escrow.ata)
      ).to.be.rejectedWith(splToken.TokenAccountNotFoundError);

      expect(results).to.be.deep.eq([
        BigInt(state.defaultDstAmount.toNumber()),
        BigInt(state.defaultSrcAmount.toNumber()),
        -BigInt(state.defaultDstAmount.toNumber()),
      ]);
    });

    it("Execute the trade with tokens => native tokens", async () => {
      const escrow = await state.initEscrow({
        escrowProgram: program,
        payer,
        provider,
        dstMint: splToken.NATIVE_MINT,
      });

      const transactionPromise = () =>
        program.methods
          .fill(escrow.order_id, state.defaultSrcAmount)
          .accounts(
            state.buildAccountsDataForAccept({
              dstMint: splToken.NATIVE_MINT,
              escrow: escrow.escrow,
              escrowSrcAta: escrow.ata,
              makerDstAta:
                state.alice.atas[splToken.NATIVE_MINT.toString()].address,
              takerDstAta:
                state.bob.atas[splToken.NATIVE_MINT.toString()].address,
            })
          )
          .signers([state.bob.keypair])
          .rpc();

      const results = await trackReceivedTokenAndTx(
        provider.connection,
        [
          state.alice.atas[splToken.NATIVE_MINT.toString()].address,
          state.bob.atas[splToken.NATIVE_MINT.toString()].address,
          state.bob.atas[state.tokens[0].toString()].address,
        ],
        transactionPromise
      );
      await expect(
        splToken.getAccount(provider.connection, escrow.ata)
      ).to.be.rejectedWith(splToken.TokenAccountNotFoundError);

      expect(results).to.be.deep.eq([
        BigInt(state.defaultDstAmount.toNumber()),
        -BigInt(state.defaultDstAmount.toNumber()),
        BigInt(state.defaultSrcAmount.toNumber()),
      ]);
    });

    it("Doesn't execute the trade with exchange amount more than escow has (x_token)", async () => {
      await expect(
        program.methods
          .fill(state.escrows[0].order_id, state.defaultSrcAmount.muln(10))
          .accounts(state.buildAccountsDataForAccept({}))
          .signers([state.bob.keypair])
          .rpc()
      ).to.be.rejectedWith("Error Code: NotEnoughTokensInEscrow");
    });

    it("Check that maker's yToken account is created automatically if it wasn't initialized before", async () => {
      // burn maker's yToken and close account
      const aliceBalanceYToken = await splToken.getAccount(
        provider.connection,
        state.alice.atas[state.tokens[1].toString()].address
      );
      await splToken.burn(
        provider.connection,
        state.alice.keypair,
        state.alice.atas[state.tokens[1].toString()].address,
        state.tokens[1],
        state.alice.keypair,
        aliceBalanceYToken.amount,
        []
      );
      await splToken.closeAccount(
        provider.connection,
        state.alice.keypair,
        state.alice.atas[state.tokens[1].toString()].address,
        state.alice.keypair.publicKey,
        state.alice.keypair.publicKey,
        []
      );
      // calc maker's yToken ata
      const aliceAtaYToken = await splToken.getAssociatedTokenAddress(
        state.tokens[1],
        state.alice.keypair.publicKey
      );

      // Check that token account doesn't exist before executing the trade
      try {
        await splToken.getAccount(provider.connection, aliceAtaYToken);
        chai.assert(false);
      } catch (e) {
        expect(e.toString().includes("TokenAccountNotFoundError"));
      }

      await program.methods
        .fill(state.escrows[0].order_id, state.defaultSrcAmount)
        .accounts(
          state.buildAccountsDataForAccept({
            makerDstAta: aliceAtaYToken,
          })
        )
        .signers([state.bob.keypair])
        .rpc();

      // Check that token account exists after trade and has expected balance
      const aliceYAta = await splToken.getAccount(
        provider.connection,
        aliceAtaYToken
      );
      expect(aliceYAta.amount).to.be.eq(
        BigInt(state.defaultDstAmount.toNumber())
      );
    });

    // TODO uncomment after receiver wallet initialization will be implemented

    // it.only("Check that taker's xToken account is created automatically if it wasn't initialized before", async () => {
    //   // burn taker's xToken and close account
    //   const state.bobBalanceXToken = await splToken.getAccount(provider.connection, state.bob.atas[state.tokens[0].toString()].address);
    //   await splToken.burn(provider.connection, state.bob.keypair, state.bob.atas[state.tokens[0].toString()].address, state.tokens[0], state.bob.keypair, state.bobBalanceXToken.amount, []);
    //   await splToken.closeAccount(provider.connection, state.bob.keypair, state.bob.atas[state.tokens[0].toString()].address, state.bob.keypair.publicKey, state.bob.keypair.publicKey, []);
    //   // calc takers's xToken ata
    //   const state.bobAtaXToken = await splToken.getAssociatedTokenAddress(state.tokens[0], state.bob.keypair.publicKey);

    //   // Check that token account doesn't exist before executing the trade
    //   try {
    //     await splToken.getAccount(provider.connection, state.bobAtaXToken);
    //     chai.assert(false);
    //   } catch (e) {
    //     expect(e.toString().includes("TokenAccountNotFoundError"));
    //   }

    //   await program.methods.fill(state.escrows[0].order_id, state.defaultSrcAmount)
    //   .accounts({
    //     taker: state.bob.keypair.publicKey,
    //     maker: state.alice.keypair.publicKey,
    //     srcMint: state.tokens[0],
    //     dstMint: state.tokens[1],
    //     escrow: state.escrows[0].escrow,
    //     escrowSrcAta: state.escrows[0].ata,
    //     makerDstAta: state.alice.atas[state.tokens[1].toString()].address,
    //     takerSrcAta: state.bobAtaXToken,
    //     takerDstAta: state.bob.atas[state.tokens[1].toString()].address,
    //     solReceiver: state.alice.keypair.publicKey,
    //     tokenProgram: splToken.TOKEN_PROGRAM_ID,
    //     associatedTokenProgram: splToken.ASSOCIATED_TOKEN_PROGRAM_ID,
    //     systemProgram: anchor.web3.SystemProgram.programId,
    //   })
    //   .signers([state.bob.keypair])
    //   .rpc();

    //   // Check that token account exists after trade and has expected balance
    //   const state.bobXAta = await splToken.getAccount(provider.connection, state.bobAtaXToken);
    //   expect(bobXAta.amount).to.be.eq(BigInt(state.defaultSrcAmount.toNumber()));
    // });

    // TODO: Add a test for the case of accepting an expired order

    it("Fails to init if escrow has been initialized", async () => {
      const order_id = state.increaseOrderID();
      const [escrow] = anchor.web3.PublicKey.findProgramAddressSync(
        [
          anchor.utils.bytes.utf8.encode("escrow"),
          state.alice.keypair.publicKey.toBuffer(),
          numberToBuffer(order_id, 4),
        ],
        program.programId
      );

      await program.methods
        .initialize(
          order_id,
          state.defaultExpirationTime,
          state.defaultSrcAmount,
          state.defaultDstAmount,
          state.defaultTraits,
          state.alice.keypair.publicKey,
          state.alice.keypair.publicKey
        )
        .accountsPartial({
          maker: state.alice.keypair.publicKey,
          srcMint: state.tokens[0],
          dstMint: state.tokens[1],
          escrow: escrow,
          authorizedUser: null,
        })
        .signers([state.alice.keypair])
        .rpc();

      await expect(
        program.methods
          .initialize(
            order_id,
            state.defaultExpirationTime,
            state.defaultSrcAmount,
            state.defaultDstAmount,
            state.defaultTraits,
            state.alice.keypair.publicKey,
            state.alice.keypair.publicKey
          )
          .accountsPartial({
            maker: state.alice.keypair.publicKey,
            srcMint: state.tokens[0],
            dstMint: state.tokens[1],
            escrow: escrow,
            authorizedUser: null,
          })
          .signers([state.alice.keypair])
          .rpc()
      ).to.be.rejectedWith("already in use");
    });

    it("Doesn't execute the trade with the wrong order_id", async () => {
      await expect(
        program.methods
          .fill(state.escrows[1].order_id, state.defaultSrcAmount)
          .accounts(state.buildAccountsDataForAccept({}))
          .signers([state.bob.keypair])
          .rpc()
      ).to.be.rejectedWith("Error Code: ConstraintSeeds");
    });

    it("Doesn't execute the trade with the wrong escrow ata", async () => {
      await expect(
        program.methods
          .fill(state.escrows[0].order_id, state.defaultSrcAmount)
          .accounts(
            state.buildAccountsDataForAccept({
              escrowSrcAta: state.escrows[1].ata,
            })
          )
          .signers([state.bob.keypair])
          .rpc()
      ).to.be.rejectedWith("Error Code: ConstraintTokenOwner");
    });

    it("Doesn't execute the trade with the wrong dstMint", async () => {
      await expect(
        program.methods
          .fill(state.escrows[0].order_id, state.defaultSrcAmount)
          .accounts(
            state.buildAccountsDataForAccept({
              dstMint: state.tokens[0],
            })
          )
          .signers([state.bob.keypair])
          .rpc()
      ).to.be.rejectedWith("Error Code: ConstraintTokenMint");
    });

    it("Doesn't execute the trade with the wrong maker receiver", async () => {
      await expect(
        program.methods
          .fill(state.escrows[0].order_id, state.defaultSrcAmount)
          .accounts(
            state.buildAccountsDataForAccept({
              makerReceiver: state.charlie.keypair.publicKey,
              makerDstAta:
                state.charlie.atas[state.tokens[1].toString()].address,
            })
          )
          .signers([state.bob.keypair])
          .rpc()
      ).to.be.rejectedWith("Error Code: SellerReceiverMismatch");
    });

    it("Execute the multiple trades", async () => {
      let transactionPromise = () =>
        program.methods
          .fill(state.escrows[0].order_id, state.defaultSrcAmount.divn(2))
          .accounts(state.buildAccountsDataForAccept({}))
          .signers([state.bob.keypair])
          .rpc();

      let results = await trackReceivedTokenAndTx(
        provider.connection,
        [
          state.escrows[0].ata,
          state.alice.atas[state.tokens[1].toString()].address,
          state.bob.atas[state.tokens[0].toString()].address,
          state.bob.atas[state.tokens[1].toString()].address,
        ],
        transactionPromise
      );

      expect(results).to.be.deep.eq([
        -BigInt(state.defaultSrcAmount.divn(2).toNumber()),
        BigInt(state.defaultDstAmount.divn(2).toNumber()),
        BigInt(state.defaultSrcAmount.divn(2).toNumber()),
        -BigInt(state.defaultDstAmount.divn(2).toNumber()),
      ]);

      // Second trade
      transactionPromise = () =>
        program.methods
          .fill(state.escrows[0].order_id, state.defaultSrcAmount.divn(2))
          .accounts(state.buildAccountsDataForAccept({}))
          .signers([state.bob.keypair])
          .rpc();

      results = await trackReceivedTokenAndTx(
        provider.connection,
        [
          state.alice.atas[state.tokens[1].toString()].address,
          state.bob.atas[state.tokens[0].toString()].address,
          state.bob.atas[state.tokens[1].toString()].address,
        ],
        transactionPromise
      );
      await expect(
        splToken.getAccount(provider.connection, state.escrows[0].ata)
      ).to.be.rejectedWith(splToken.TokenAccountNotFoundError);

      expect(results).to.be.deep.eq([
        BigInt(state.defaultDstAmount.divn(2).toNumber()),
        BigInt(state.defaultSrcAmount.divn(2).toNumber()),
        -BigInt(state.defaultDstAmount.divn(2).toNumber()),
      ]);
    });

    it("Execute the multiple trades, rounding", async () => {
      const _srcAmount = new anchor.BN(101);
      const _dstAmount = new anchor.BN(101);
      const escrow = await state.initEscrow({
        escrowProgram: program,
        payer,
        provider,
        srcAmount: _srcAmount,
        dstAmount: _dstAmount,
      });

      let transactionPromise = () =>
        program.methods
          .fill(escrow.order_id, _srcAmount.divn(2))
          .accounts(
            state.buildAccountsDataForAccept({
              escrow: escrow.escrow,
              escrowSrcAta: escrow.ata,
            })
          )
          .signers([state.bob.keypair])
          .rpc();

      let results = await trackReceivedTokenAndTx(
        provider.connection,
        [
          escrow.ata,
          state.alice.atas[state.tokens[1].toString()].address,
          state.bob.atas[state.tokens[0].toString()].address,
          state.bob.atas[state.tokens[1].toString()].address,
        ],
        transactionPromise
      );

      expect(results).to.be.deep.eq([
        -BigInt(_srcAmount.divn(2).toNumber()),
        BigInt(_dstAmount.divn(2).toNumber()),
        BigInt(_srcAmount.divn(2).toNumber()),
        -BigInt(_dstAmount.divn(2).toNumber()),
      ]);

      // Second trade
      transactionPromise = () =>
        program.methods
          .fill(escrow.order_id, _srcAmount.divn(2))
          .accounts(
            state.buildAccountsDataForAccept({
              escrow: escrow.escrow,
              escrowSrcAta: escrow.ata,
            })
          )
          .signers([state.bob.keypair])
          .rpc();

      results = await trackReceivedTokenAndTx(
        provider.connection,
        [
          escrow.ata,
          state.alice.atas[state.tokens[1].toString()].address,
          state.bob.atas[state.tokens[0].toString()].address,
          state.bob.atas[state.tokens[1].toString()].address,
        ],
        transactionPromise
      );

      expect(results).to.be.deep.eq([
        -BigInt(_srcAmount.divn(2).toNumber()),
        BigInt(_dstAmount.divn(2).toNumber()),
        BigInt(_srcAmount.divn(2).toNumber()),
        -BigInt(_dstAmount.divn(2).toNumber()),
      ]);

      // Third trade
      transactionPromise = () =>
        program.methods
          .fill(escrow.order_id, new anchor.BN(1))
          .accounts(
            state.buildAccountsDataForAccept({
              escrow: escrow.escrow,
              escrowSrcAta: escrow.ata,
            })
          )
          .signers([state.bob.keypair])
          .rpc();

      results = await trackReceivedTokenAndTx(
        provider.connection,
        [
          // escrow.ata, // escrow closed and ata not exists
          state.alice.atas[state.tokens[1].toString()].address,
          state.bob.atas[state.tokens[0].toString()].address,
          state.bob.atas[state.tokens[1].toString()].address,
        ],
        transactionPromise
      );

      expect(results).to.be.deep.eq([
        // -BigInt(new anchor.BN(1).toNumber()), // escrow closed and ata not exists
        BigInt(new anchor.BN(1).toNumber()),
        BigInt(new anchor.BN(1).toNumber()),
        -BigInt(new anchor.BN(1).toNumber()),
      ]);
    });

    it("Cancel the trade", async () => {
      const transactionPromise = () =>
        program.methods
          .cancel(state.escrows[0].order_id)
          .accountsPartial({
            maker: state.alice.keypair.publicKey,
            srcMint: state.tokens[0],
            escrow: state.escrows[0].escrow,
            solReceiver: state.alice.keypair.publicKey,
          })
          .signers([state.alice.keypair])
          .rpc();

      const results = await trackReceivedTokenAndTx(
        provider.connection,
        [state.alice.atas[state.tokens[0].toString()].address],
        transactionPromise
      );

      expect(results).to.be.deep.eq([
        BigInt(state.defaultSrcAmount.toNumber()),
      ]);
    });

    it("Cancel the trade with native tokens", async () => {
      const escrow = await state.initEscrow({
        escrowProgram: program,
        payer,
        provider,
        srcMint: splToken.NATIVE_MINT,
      });

      const transactionPromise = () =>
        program.methods
          .cancel(escrow.order_id)
          .accountsPartial({
            maker: state.alice.keypair.publicKey,
            srcMint: splToken.NATIVE_MINT,
            escrow: escrow.escrow,
            solReceiver: state.alice.keypair.publicKey,
          })
          .signers([state.alice.keypair])
          .rpc();

      const results = await trackReceivedTokenAndTx(
        provider.connection,
        [state.alice.atas[splToken.NATIVE_MINT.toString()].address],
        transactionPromise
      );

      expect(results).to.be.deep.eq([
        BigInt(state.defaultSrcAmount.toNumber()),
      ]);
    });

    it("Doesn't cancel the trade with the wrong order_id", async () => {
      await expect(
        program.methods
          .cancel(state.escrows[1].order_id)
          .accountsPartial({
            maker: state.alice.keypair.publicKey,
            srcMint: state.tokens[0],
            escrow: state.escrows[0].escrow,
            solReceiver: state.alice.keypair.publicKey,
          })
          .signers([state.alice.keypair])
          .rpc()
      ).to.be.rejectedWith("Error Code: ConstraintSeeds");
    });

    it("Doesn't cancel the trade with the wrong escrow ata", async () => {
      await expect(
        program.methods
          .cancel(state.escrows[0].order_id)
          .accountsPartial({
            maker: state.alice.keypair.publicKey,
            srcMint: state.tokens[0],
            escrow: state.escrows[0].escrow,
            escrowSrcAta: state.escrows[1].ata,
            solReceiver: state.alice.keypair.publicKey,
          })
          .signers([state.alice.keypair])
          .rpc()
      ).to.be.rejectedWith("Error Code: ConstraintTokenOwner");
    });

    it("Doesn't cancel the trade with the wrong maker", async () => {
      await expect(
        program.methods
          .cancel(state.escrows[0].order_id)
          .accountsPartial({
            maker: state.charlie.keypair.publicKey,
            srcMint: state.tokens[0],
            escrow: state.escrows[0].escrow,
            solReceiver: state.alice.keypair.publicKey,
          })
          .signers([state.charlie.keypair])
          .rpc()
      ).to.be.rejectedWith("Error Code: ConstraintSeeds");
    });

    it("Fails when taker does not match the authorized user", async () => {
      const escrow = await state.initEscrow({
        escrowProgram: program,
        payer,
        provider,
        authorizedUser: state.charlie.keypair.publicKey,
      });

      await expect(
        program.methods
          .fill(escrow.order_id, state.defaultSrcAmount.divn(2))
          .accounts(
            state.buildAccountsDataForAccept({
              escrow: escrow.escrow,
              escrowSrcAta: escrow.ata,
            })
          )
          .signers([state.bob.keypair])
          .rpc()
      ).to.be.rejectedWith("Error Code: PrivateOrder");
    });

    it("Execute the trade and close escow after fullfill", async () => {
      const escrow = await state.initEscrow({
        escrowProgram: program,
        payer,
        provider,
        sol_receiver: state.charlie.keypair.publicKey,
      });

      const solReceiverNativeTokenBalanceBefore =
        await provider.connection.getBalance(state.charlie.keypair.publicKey);
      await program.methods
        .fill(escrow.order_id, state.defaultSrcAmount)
        .accounts(
          state.buildAccountsDataForAccept({
            escrow: escrow.escrow,
            escrowSrcAta: escrow.ata,
            solReceiver: state.charlie.keypair.publicKey,
          })
        )
        .signers([state.bob.keypair])
        .rpc();

      const solReceiverNativeTokenBalanceAfter =
        await provider.connection.getBalance(state.charlie.keypair.publicKey);
      // check that escrow closed and native tokens sent to maker
      expect(solReceiverNativeTokenBalanceAfter).to.be.gt(
        solReceiverNativeTokenBalanceBefore
      );
    });

    it("Doesn't fill partial fill with allow_partial_fill=false", async () => {
      const escrow = await state.initEscrow({
        escrowProgram: program,
        payer,
        provider,
        escrow_traits: buildEscrowTraits({ isPartialFill: false }),
      });

      await expect(
        program.methods
          .fill(escrow.order_id, state.defaultSrcAmount.divn(2))
          .accounts(
            state.buildAccountsDataForAccept({
              escrow: escrow.escrow,
              escrowSrcAta: escrow.ata,
            })
          )
          .signers([state.bob.keypair])
          .rpc()
      ).to.be.rejectedWith("Error Code: PartialFillNotAllowed");
    });
  });

  describe("Optional tests", () => {
    it("Doesn't execute the trade with the wrong maker's ata", async () => {
      await expect(
        program.methods
          .fill(state.escrows[0].order_id, state.defaultSrcAmount)
          .accounts(
            state.buildAccountsDataForAccept({
              makerDstAta:
                state.charlie.atas[state.tokens[1].toString()].address,
            })
          )
          .signers([state.bob.keypair])
          .rpc()
      ).to.be.rejectedWith("Error Code: ConstraintTokenOwner");
    });

    it("Doesn't execute the trade with the wrong token", async () => {
      await expect(
        program.methods
          .fill(state.escrows[0].order_id, state.defaultSrcAmount)
          .accounts(
            state.buildAccountsDataForAccept({
              srcMint: state.tokens[1],
            })
          )
          .signers([state.bob.keypair])
          .rpc()
      ).to.be.rejectedWith("Error Code: ConstraintAssociated");
    });
  });

  describe("Multiple escrows", () => {
    it("Double fill", async () => {
      const transactionPromise = async () => {
        await program.methods
          .fill(state.escrows[0].order_id, state.defaultSrcAmount)
          .accounts(state.buildAccountsDataForAccept({}))
          .signers([state.bob.keypair])
          .rpc();

        await program.methods
          .fill(state.escrows[1].order_id, state.defaultSrcAmount)
          .accounts(
            state.buildAccountsDataForAccept({
              taker: state.charlie.keypair.publicKey,
              escrow: state.escrows[1].escrow,
              escrowSrcAta: state.escrows[1].ata,
              takerSrcAta:
                state.charlie.atas[state.tokens[0].toString()].address,
              takerDstAta:
                state.charlie.atas[state.tokens[1].toString()].address,
            })
          )
          .signers([state.charlie.keypair])
          .rpc();
      };

      const results = await trackReceivedTokenAndTx(
        provider.connection,
        [
          state.alice.atas[state.tokens[1].toString()].address,
          state.bob.atas[state.tokens[0].toString()].address,
          state.bob.atas[state.tokens[1].toString()].address,
          state.charlie.atas[state.tokens[0].toString()].address,
          state.charlie.atas[state.tokens[1].toString()].address,
        ],
        transactionPromise
      );
      await expect(
        splToken.getAccount(provider.connection, state.escrows[0].ata)
      ).to.be.rejectedWith(splToken.TokenAccountNotFoundError);
      await expect(
        splToken.getAccount(provider.connection, state.escrows[1].ata)
      ).to.be.rejectedWith(splToken.TokenAccountNotFoundError);

      expect(results).to.be.deep.eq([
        BigInt(state.defaultDstAmount.toNumber()) * 2n,
        BigInt(state.defaultSrcAmount.toNumber()),
        -BigInt(state.defaultDstAmount.toNumber()),
        BigInt(state.defaultSrcAmount.toNumber()),
        -BigInt(state.defaultDstAmount.toNumber()),
      ]);
    });

    it("Execute the trade with allow_multiple_fill=false only once", async () => {
      const escrow = await state.initEscrow({
        escrowProgram: program,
        payer,
        provider,
        escrow_traits: buildEscrowTraits({ isMultipleFill: false }),
      });

      const transactionPromise = () =>
        program.methods
          .fill(escrow.order_id, state.defaultSrcAmount.divn(10))
          .accounts(
            state.buildAccountsDataForAccept({
              escrow: escrow.escrow,
              escrowSrcAta: escrow.ata,
            })
          )
          .signers([state.bob.keypair])
          .rpc();

      await transactionPromise();
      await expect(transactionPromise()).to.be.rejectedWith(
        "Error Code: AccountNotInitialized"
      );
    });

    it("Execute the trade with allow_multiple_fill=false should close escrow", async () => {
      const escrow = await state.initEscrow({
        escrowProgram: program,
        payer,
        provider,
        escrow_traits: buildEscrowTraits({ isMultipleFill: false }),
      });

      const makerNativeTokenBalanceBefore =
        await provider.connection.getBalance(state.alice.keypair.publicKey);
      await program.methods
        .fill(escrow.order_id, state.defaultSrcAmount.divn(10))
        .accounts(
          state.buildAccountsDataForAccept({
            escrow: escrow.escrow,
            escrowSrcAta: escrow.ata,
          })
        )
        .signers([state.bob.keypair])
        .rpc();

      const makerNativeTokenBalanceAfter = await provider.connection.getBalance(
        state.alice.keypair.publicKey
      );
      // check that escrow closed and native tokens sent to maker
      expect(makerNativeTokenBalanceAfter).to.be.gt(
        makerNativeTokenBalanceBefore
      );
      await assertThrowsAsync(
        () => splToken.getAccount(provider.connection, escrow.ata),
        "TokenAccountNotFoundError"
      );
      expect(await provider.connection.getBalance(escrow.escrow)).to.be.eq(0);
    });
  });
});
