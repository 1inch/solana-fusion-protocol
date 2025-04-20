import * as anchor from "@coral-xyz/anchor";
import * as splToken from "@solana/spl-token";
import NodeWallet from "@coral-xyz/anchor/dist/cjs/nodewallet";
import chai, { expect } from "chai";
import chaiAsPromised from "chai-as-promised";
import {
  TestState,
  createTokens,
  createAtasUsers,
  createWhitelistedAccount,
  debugLog,
  mintTokens,
  removeWhitelistedAccount,
  trackReceivedTokenAndTx,
  ReducedOrderConfig,
} from "../utils/utils";
import { Whitelist } from "../../target/types/whitelist";
import FUSION_SWAP_NATIVE_IDL from "../../idl/fusion_swap_native.json";
import { FusionSwapNative } from "../../idl/fusion_swap_native";
import { sendAndConfirmTransaction } from "@solana/web3.js";
import { Program } from "@coral-xyz/anchor";
import { calculateOrderHash } from "../../scripts/utils";
chai.use(chaiAsPromised);

const ESCROW_ERRORS = {
  ConstraintTokenMint: 2014,
  ConstraintSigner: 2002,
  AccountNotMutable: 3006,
  ConstraintOwner: 2004,
  ConstraintTokenOwner: 2015,
  ConstraintMintTokenProgram: 2022,
  ConstraintSeeds: 2006,
  ConstraintAddress: 2012,
  AccountNotAssociatedTokenAccount: 3014,
  InconsistentNativeDstTrait: 6000,
  InvalidAmount: 6001,
  MissingMakerDstAta: 6002,
  NotEnoughTokensInEscrow: 6003,
  OrderExpired: 6004,
  InvalidEstimatedTakingAmount: 6005,
  InvalidProtocolSurplusFee: 6006,
  InconsistentProtocolFeeConfig: 6007,
  InconsistentIntegratorFeeConfig: 6008,
};

function errorCodeHex(error: string): string {
  const errorCode = ESCROW_ERRORS[error as keyof typeof ESCROW_ERRORS];

  if (errorCode === undefined) {
    throw new Error(`Error code not found for ${error}`);
  }

  return `0x${errorCode.toString(16)}`;
}

describe.skip("Fusion Swap Native", () => {
  const provider = anchor.AnchorProvider.env();
  anchor.setProvider(provider);

  const program = new Program<FusionSwapNative>(FUSION_SWAP_NATIVE_IDL, {
    connection: provider.connection,
  });
  const whitelistProgram = anchor.workspace
    .Whitelist as anchor.Program<Whitelist>;

  const payer = (provider.wallet as NodeWallet).payer;
  debugLog(`Payer ::`, payer.publicKey.toString());

  let state: TestState;

  before(async () => {
    // Since 'fusion-swap-native' is not an Anchor program, we need to deploy it to localnet manually.
    //
    // We also specify the path to fixed keypair file to make these tests work on CI and different machines,
    // due to the fact that program id specified in hardcoded IDL is hardcoded too, and thus needs to be preserved.
    const deployOutput = require("child_process").execSync(
      "solana program deploy target/deploy/fusion_swap_native.so -u localhost --program-id fusion_swap_native-keypair.json"
    );
    console.log(deployOutput.toString());

    state = await TestState.anchorCreate(provider, payer, { tokensNums: 3 });
  });

  beforeEach(async () => {
    state.escrows = [];
    for (let i = 0; i < 2; ++i) {
      const escrow = await state.createEscrow({
        escrowProgram: program,
        payer,
        provider,
      });
      state.escrows.push(escrow);
      debugLog(`Escrow_${escrow.orderConfig.id} ::`, escrow.escrow.toString());
      debugLog(`escrowAta_${escrow.orderConfig.id} ::`, escrow.ata.toString());
    }
  });

  describe("Single escrow", () => {
    it("Creates escrow src ata", async () => {
      const orderConfig = state.orderConfig({});

      const [escrow] = anchor.web3.PublicKey.findProgramAddressSync(
        [
          anchor.utils.bytes.utf8.encode("escrow"),
          state.alice.keypair.publicKey.toBuffer(),
          calculateOrderHash(orderConfig),
        ],
        program.programId
      );

      await program.methods
        .create(orderConfig as ReducedOrderConfig)
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
        .transaction()
        .then((tx) =>
          sendAndConfirmTransaction(provider.connection, tx, [
            state.alice.keypair,
          ])
        );

      const escrowAtaAddr = await splToken.getAssociatedTokenAddress(
        state.tokens[0],
        escrow,
        true
      );
      const escrowSrcAta = await splToken.getAccount(
        provider.connection,
        escrowAtaAddr
      );

      expect(escrowSrcAta.amount).to.be.eq(
        BigInt(orderConfig.srcAmount.toNumber())
      );
    });

    it.skip("Execute the trade", async () => {
      const transactionPromise = () =>
        program.methods
          .fill(state.escrows[0].orderConfig.id, state.defaultSrcAmount)
          .accountsPartial(state.buildAccountsDataForFill({}))
          .signers([state.bob.keypair])
          .transaction()
          .then((tx) =>
            sendAndConfirmTransaction(provider.connection, tx, [
              state.bob.keypair,
            ])
          );

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

    it.skip("Execute the trade with different maker's receiver", async () => {
      const escrow = await state.createEscrow({
        escrowProgram: program,
        payer,
        provider,
        orderConfig: state.orderConfig({
          receiver: state.charlie.keypair.publicKey,
        }),
      });
      const transactionPromise = () =>
        program.methods
          .fill(escrow.orderConfig.id, state.defaultSrcAmount)
          .accountsPartial(
            state.buildAccountsDataForFill({
              makerReceiver: state.charlie.keypair.publicKey,
              escrow: escrow.escrow,
              escrowSrcAta: escrow.ata,
              makerDstAta:
                state.charlie.atas[state.tokens[1].toString()].address,
            })
          )
          .signers([state.bob.keypair])
          .transaction()
          .then((tx) =>
            sendAndConfirmTransaction(provider.connection, tx, [
              state.bob.keypair,
            ])
          );

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

    it.skip("Execute the trade without u64 overflow", async () => {
      // amount * amount is greater than u64::max
      const amount = new anchor.BN(10 * Math.pow(10, 9));

      await mintTokens(
        state.tokens[0],
        state.alice,
        amount.toNumber(),
        provider,
        payer
      );

      await mintTokens(
        state.tokens[1],
        state.bob,
        amount.toNumber(),
        provider,
        payer
      );

      const escrow = await state.createEscrow({
        escrowProgram: program,
        payer,
        provider,
        orderConfig: state.orderConfig({
          srcAmount: amount,
          minDstAmount: amount,
          estimatedDstAmount: amount,
        }),
      });

      const transactionPromise = () =>
        program.methods
          .fill(escrow.orderConfig.id, amount)
          .accountsPartial(
            state.buildAccountsDataForFill({
              escrow: escrow.escrow,
              escrowSrcAta: escrow.ata,
            })
          )
          .signers([state.bob.keypair])
          .transaction()
          .then((tx) =>
            sendAndConfirmTransaction(provider.connection, tx, [
              state.bob.keypair,
            ])
          );

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
        splToken.getAccount(provider.connection, escrow.ata)
      ).to.be.rejectedWith(splToken.TokenAccountNotFoundError);

      expect(results).to.be.deep.eq([
        BigInt(amount.toNumber()),
        BigInt(amount.toNumber()),
        -BigInt(amount.toNumber()),
      ]);

      // Burn excess tokens to not affect the global state
      await splToken.burn(
        provider.connection,
        state.alice.keypair,
        state.alice.atas[state.tokens[1].toString()].address,
        state.tokens[1],
        state.alice.keypair,
        amount.toNumber()
      );

      await splToken.burn(
        provider.connection,
        state.bob.keypair,
        state.bob.atas[state.tokens[0].toString()].address,
        state.tokens[0],
        state.bob.keypair,
        amount.toNumber()
      );
    });

    it.skip("Execute the trade with different taker's receiver wallet", async () => {
      const transactionPromise = () =>
        program.methods
          .fill(state.escrows[0].orderConfig.id, state.defaultSrcAmount)
          .accountsPartial(
            state.buildAccountsDataForFill({
              takerSrcAta:
                state.charlie.atas[state.tokens[0].toString()].address,
            })
          )
          .signers([state.bob.keypair])
          .transaction()
          .then((tx) =>
            sendAndConfirmTransaction(provider.connection, tx, [
              state.bob.keypair,
            ])
          );

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

    it.skip("Doesn't execute the trade when maker's token account belongs to wrong mint", async () => {
      await expect(
        program.methods
          .fill(state.escrows[0].orderConfig.id, state.defaultSrcAmount)
          .accountsPartial(
            state.buildAccountsDataForFill({
              makerDstAta: state.alice.atas[state.tokens[2].toString()].address,
            })
          )
          .signers([state.bob.keypair])
          .transaction()
          .then((tx) =>
            sendAndConfirmTransaction(provider.connection, tx, [
              state.bob.keypair,
            ])
          )
      ).to.be.rejectedWith("Error Code: ConstraintTokenMint");
    });

    it.skip("Execute the trade with native tokens => tokens", async () => {
      const escrow = await state.createEscrow({
        escrowProgram: program,
        payer,
        provider,
        orderConfig: {
          srcMint: splToken.NATIVE_MINT,
        },
      });

      const transactionPromise = () =>
        program.methods
          .fill(escrow.orderConfig.id, state.defaultSrcAmount)
          .accountsPartial(
            state.buildAccountsDataForFill({
              srcMint: splToken.NATIVE_MINT,
              escrow: escrow.escrow,
              escrowSrcAta: escrow.ata,
              takerSrcAta:
                state.bob.atas[splToken.NATIVE_MINT.toString()].address,
            })
          )
          .signers([state.bob.keypair])
          .transaction()
          .then((tx) =>
            sendAndConfirmTransaction(provider.connection, tx, [
              state.bob.keypair,
            ])
          );

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

    it.skip("Execute the trade with tokens => native tokens", async () => {
      const escrow = await state.createEscrow({
        escrowProgram: program,
        payer,
        provider,
        orderConfig: {
          dstMint: splToken.NATIVE_MINT,
        },
      });

      const transactionPromise = () =>
        program.methods
          .fill(escrow.orderConfig.id, state.defaultSrcAmount)
          .accountsPartial(
            state.buildAccountsDataForFill({
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
          .transaction()
          .then((tx) =>
            sendAndConfirmTransaction(provider.connection, tx, [
              state.bob.keypair,
            ])
          );

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

    describe.skip("Token 2022", () => {
      before(async () => {
        const tokens = await createTokens(
          2,
          provider,
          payer,
          splToken.TOKEN_2022_PROGRAM_ID
        );
        state.tokens.push(...tokens);

        const usersWithToken2022Atas = await createAtasUsers(
          [state.alice.keypair, state.bob.keypair],
          tokens,
          provider,
          payer,
          splToken.TOKEN_2022_PROGRAM_ID
        );
        state.alice.atas = {
          ...state.alice.atas,
          ...usersWithToken2022Atas[0].atas,
        };
        state.bob.atas = {
          ...state.bob.atas,
          ...usersWithToken2022Atas[1].atas,
        };

        await mintTokens(
          tokens[0],
          state.alice,
          100_000_000,
          provider,
          payer,
          splToken.TOKEN_2022_PROGRAM_ID
        );
        await mintTokens(
          tokens[1],
          state.bob,
          100_000_000,
          provider,
          payer,
          splToken.TOKEN_2022_PROGRAM_ID
        );
      });

      it("Execute trade with SPL Token -> Token 2022", async () => {
        const dstTokenProgram = splToken.TOKEN_2022_PROGRAM_ID;
        const dstMint = state.tokens[state.tokens.length - 1]; // Token 2022
        const makerDstAta = state.alice.atas[dstMint.toString()].address;
        const takerDstAta = state.bob.atas[dstMint.toString()].address;
        const escrow = await state.createEscrow({
          escrowProgram: program,
          payer,
          provider,
          orderConfig: {
            dstMint,
          },
        });

        const transactionPromise = () =>
          program.methods
            .fill(escrow.orderConfig.id, state.defaultSrcAmount)
            .accountsPartial({
              ...state.buildAccountsDataForFill({
                escrow: escrow.escrow,
                escrowSrcAta: escrow.ata,
                dstMint,
                makerDstAta,
                takerDstAta,
                dstTokenProgram,
              }),
            })
            .signers([state.bob.keypair])
            .transaction()
            .then((tx) =>
              sendAndConfirmTransaction(provider.connection, tx, [
                state.bob.keypair,
              ])
            );

        const results = await trackReceivedTokenAndTx(
          provider.connection,
          [
            { publicKey: makerDstAta, programId: dstTokenProgram },
            {
              publicKey: state.bob.atas[state.tokens[0].toString()].address,
              programId: splToken.TOKEN_PROGRAM_ID,
            },
            { publicKey: takerDstAta, programId: dstTokenProgram },
          ],
          transactionPromise
        );

        expect(results).to.be.deep.eq([
          BigInt(state.defaultDstAmount.toNumber()),
          BigInt(state.defaultSrcAmount.toNumber()),
          -BigInt(state.defaultDstAmount.toNumber()),
        ]);
      });

      it("Execute trade with Token 2022 -> SPL Token", async () => {
        const srcTokenProgram = splToken.TOKEN_2022_PROGRAM_ID;
        const srcMint = state.tokens[state.tokens.length - 2]; // Token 2022
        const takerSrcAta = state.bob.atas[srcMint.toString()].address;

        const escrow = await state.createEscrow({
          escrowProgram: program,
          payer,
          provider,
          orderConfig: {
            srcMint,
          },
          srcTokenProgram,
        });

        const transactionPromise = () =>
          program.methods
            .fill(escrow.orderConfig.id, state.defaultSrcAmount)
            .accountsPartial({
              ...state.buildAccountsDataForFill({
                escrow: escrow.escrow,
                escrowSrcAta: escrow.ata,
                srcMint,
                takerSrcAta,
                srcTokenProgram,
              }),
            })
            .signers([state.bob.keypair])
            .transaction()
            .then((tx) =>
              sendAndConfirmTransaction(provider.connection, tx, [
                state.bob.keypair,
              ])
            );

        const results = await trackReceivedTokenAndTx(
          provider.connection,
          [
            {
              publicKey: state.alice.atas[state.tokens[1].toString()].address,
              programId: splToken.TOKEN_PROGRAM_ID,
            },
            { publicKey: takerSrcAta, programId: srcTokenProgram },
            {
              publicKey: state.bob.atas[state.tokens[1].toString()].address,
              programId: splToken.TOKEN_PROGRAM_ID,
            },
          ],
          transactionPromise
        );

        expect(results).to.be.deep.eq([
          BigInt(state.defaultDstAmount.toNumber()),
          BigInt(state.defaultSrcAmount.toNumber()),
          -BigInt(state.defaultDstAmount.toNumber()),
        ]);
      });

      it("Execute trade between two Token 2022 tokens", async () => {
        const tokenProgram = splToken.TOKEN_2022_PROGRAM_ID;
        const srcMint = state.tokens[state.tokens.length - 2]; // First Token 2022
        const dstMint = state.tokens[state.tokens.length - 1]; // Second Token 2022
        const makerDstAta = state.alice.atas[dstMint.toString()].address;
        const takerSrcAta = state.bob.atas[srcMint.toString()].address;
        const takerDstAta = state.bob.atas[dstMint.toString()].address;

        const escrow = await state.createEscrow({
          escrowProgram: program,
          payer,
          provider,
          orderConfig: {
            srcMint,
            dstMint,
          },
          srcTokenProgram: tokenProgram,
        });

        const transactionPromise = () =>
          program.methods
            .fill(escrow.orderConfig.id, state.defaultSrcAmount)
            .accountsPartial({
              ...state.buildAccountsDataForFill({
                escrow: escrow.escrow,
                escrowSrcAta: escrow.ata,
                srcMint,
                dstMint,
                makerDstAta,
                takerSrcAta,
                takerDstAta,
                srcTokenProgram: tokenProgram,
                dstTokenProgram: tokenProgram,
              }),
            })
            .signers([state.bob.keypair])
            .transaction()
            .then((tx) =>
              sendAndConfirmTransaction(provider.connection, tx, [
                state.bob.keypair,
              ])
            );

        const results = await trackReceivedTokenAndTx(
          provider.connection,
          [
            { publicKey: makerDstAta, programId: tokenProgram },
            { publicKey: takerSrcAta, programId: tokenProgram },
            { publicKey: takerDstAta, programId: tokenProgram },
          ],
          transactionPromise
        );

        expect(results).to.be.deep.eq([
          BigInt(state.defaultDstAmount.toNumber()),
          BigInt(state.defaultSrcAmount.toNumber()),
          -BigInt(state.defaultDstAmount.toNumber()),
        ]);
      });

      it("Cancel escrow with Token 2022", async () => {
        const tokenProgram = splToken.TOKEN_2022_PROGRAM_ID;
        const srcMint = state.tokens[state.tokens.length - 2]; // Token 2022

        const escrow = await state.createEscrow({
          escrowProgram: program,
          payer,
          provider,
          orderConfig: {
            srcMint,
          },
          srcTokenProgram: tokenProgram,
        });

        const transactionPromise = () =>
          program.methods
            .cancel(escrow.orderConfig.id)
            .accountsPartial({
              maker: state.alice.keypair.publicKey,
              srcMint,
              escrow: escrow.escrow,
              srcTokenProgram: tokenProgram,
            })
            .signers([state.alice.keypair])
            .transaction()
            .then((tx) =>
              sendAndConfirmTransaction(provider.connection, tx, [
                state.alice.keypair,
              ])
            );

        const results = await trackReceivedTokenAndTx(
          provider.connection,
          [
            {
              publicKey: state.alice.atas[srcMint.toString()].address,
              programId: tokenProgram,
            },
          ],
          transactionPromise
        );

        expect(results).to.be.deep.eq([
          BigInt(state.defaultSrcAmount.toNumber()),
        ]);
      });
    });

    it.skip("Execute the trade with protocol fee", async () => {
      const escrow = await state.createEscrow({
        escrowProgram: program,
        payer,
        provider,
        orderConfig: state.orderConfig({
          fee: {
            protocolFee: 10000, // 10%
            protocolDstAcc:
              state.charlie.atas[state.tokens[1].toString()].address,
            integratorDstAcc: null,
          },
        }),
      });

      const transactionPromise = () =>
        program.methods
          .fill(escrow.orderConfig.id, state.defaultSrcAmount)
          .accountsPartial(
            state.buildAccountsDataForFill({
              escrow: escrow.escrow,
              escrowSrcAta: escrow.ata,
              protocolDstAcc:
                state.charlie.atas[state.tokens[1].toString()].address,
            })
          )
          .signers([state.bob.keypair])
          .transaction()
          .then((tx) =>
            sendAndConfirmTransaction(provider.connection, tx, [
              state.bob.keypair,
            ])
          );

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
        splToken.getAccount(provider.connection, escrow.ata)
      ).to.be.rejectedWith(splToken.TokenAccountNotFoundError);

      expect(results).to.be.deep.eq([
        BigInt((state.defaultDstAmount.toNumber() * 9) / 10),
        BigInt(state.defaultSrcAmount.toNumber()),
        -BigInt(state.defaultDstAmount.toNumber()),
        BigInt(state.defaultDstAmount.toNumber() / 10),
      ]);
    });

    it.skip("Execute the trade with integrator fee", async () => {
      const escrow = await state.createEscrow({
        escrowProgram: program,
        payer,
        provider,
        orderConfig: state.orderConfig({
          fee: {
            integratorFee: 15000, // 15%
            protocolDstAcc: null,
            integratorDstAcc:
              state.charlie.atas[state.tokens[1].toString()].address,
          },
        }),
      });

      const transactionPromise = () =>
        program.methods
          .fill(escrow.orderConfig.id, state.defaultSrcAmount)
          .accountsPartial(
            state.buildAccountsDataForFill({
              escrow: escrow.escrow,
              escrowSrcAta: escrow.ata,
              integratorDstAcc:
                state.charlie.atas[state.tokens[1].toString()].address,
            })
          )
          .signers([state.bob.keypair])
          .transaction()
          .then((tx) =>
            sendAndConfirmTransaction(provider.connection, tx, [
              state.bob.keypair,
            ])
          );

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
        splToken.getAccount(provider.connection, escrow.ata)
      ).to.be.rejectedWith(splToken.TokenAccountNotFoundError);

      expect(results).to.be.deep.eq([
        BigInt(Math.ceil((state.defaultDstAmount.toNumber() * 85) / 100)),
        BigInt(state.defaultSrcAmount.toNumber()),
        -BigInt(state.defaultDstAmount.toNumber()),
        BigInt(Math.floor((state.defaultDstAmount.toNumber() * 15) / 100)),
      ]);
    });

    it.skip("Doesn't execute the trade with exchange amount more than escow has (x_token)", async () => {
      await expect(
        program.methods
          .fill(
            state.escrows[0].orderConfig.id,
            state.defaultSrcAmount.muln(10)
          )
          .accountsPartial(state.buildAccountsDataForFill({}))
          .signers([state.bob.keypair])
          .transaction()
          .then((tx) =>
            sendAndConfirmTransaction(provider.connection, tx, [
              state.bob.keypair,
            ])
          )
      ).to.be.rejectedWith("Error Code: NotEnoughTokensInEscrow");
    });

    it.skip("Check that maker's yToken account is created automatically if it wasn't initialized before", async () => {
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
        .fill(state.escrows[0].orderConfig.id, state.defaultSrcAmount)
        .accountsPartial(
          state.buildAccountsDataForFill({
            makerDstAta: aliceAtaYToken,
          })
        )
        .signers([state.bob.keypair])
        .transaction()
        .then((tx) =>
          sendAndConfirmTransaction(provider.connection, tx, [
            state.bob.keypair,
          ])
        );

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

    it("Fails to create with zero src amount", async () => {
      const orderConfig = state.orderConfig({
        srcAmount: new anchor.BN(0),
      });

      const [escrow] = anchor.web3.PublicKey.findProgramAddressSync(
        [
          anchor.utils.bytes.utf8.encode("escrow"),
          state.alice.keypair.publicKey.toBuffer(),
          calculateOrderHash(orderConfig),
        ],
        program.programId
      );

      // srcAmount = 0
      await expect(
        program.methods
          .create(orderConfig as ReducedOrderConfig)
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
          .transaction()
          .then((tx) =>
            sendAndConfirmTransaction(provider.connection, tx, [
              state.alice.keypair,
            ])
          )
      ).to.be.rejectedWith(errorCodeHex("InvalidAmount"));
    });

    it("Fails to create with zero min dst amount", async () => {
      const orderConfig = state.orderConfig({
        minDstAmount: new anchor.BN(0),
      });

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
          .create(orderConfig as ReducedOrderConfig)
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
          .transaction()
          .then((tx) =>
            sendAndConfirmTransaction(provider.connection, tx, [
              state.alice.keypair,
            ])
          )
      ).to.be.rejectedWith(errorCodeHex("InvalidAmount"));
    });

    it("Fails to create if escrow has been created already", async () => {
      const orderConfig = state.orderConfig();

      const [escrow] = anchor.web3.PublicKey.findProgramAddressSync(
        [
          anchor.utils.bytes.utf8.encode("escrow"),
          state.alice.keypair.publicKey.toBuffer(),
          calculateOrderHash(orderConfig),
        ],
        program.programId
      );

      await program.methods
        .create(orderConfig as ReducedOrderConfig)
        .accountsPartial({
          maker: state.alice.keypair.publicKey,
          srcMint: state.tokens[0],
          dstMint: state.tokens[1],
          protocolDstAcc: null,
          integratorDstAcc: null,
          escrow: escrow,
          srcTokenProgram: splToken.TOKEN_PROGRAM_ID,
          makerReceiver: state.alice.keypair.publicKey,
        })
        .signers([state.alice.keypair])
        .transaction()
        .then((tx) =>
          sendAndConfirmTransaction(provider.connection, tx, [
            state.alice.keypair,
          ])
        );

      await expect(
        program.methods
          .create(orderConfig as ReducedOrderConfig)
          .accountsPartial({
            maker: state.alice.keypair.publicKey,
            srcMint: state.tokens[0],
            dstMint: state.tokens[1],
            protocolDstAcc: null,
            integratorDstAcc: null,
            escrow: escrow,
            srcTokenProgram: splToken.TOKEN_PROGRAM_ID,
            makerReceiver: state.alice.keypair.publicKey,
          })
          .signers([state.alice.keypair])
          .transaction()
          .then((tx) =>
            sendAndConfirmTransaction(provider.connection, tx, [
              state.alice.keypair,
            ])
          )
      ).to.be.rejectedWith("instruction requires an uninitialized account");
    });

    it.skip("Doesn't execute the trade with the wrong order_id", async () => {
      await expect(
        program.methods
          .fill(state.escrows[1].orderConfig.id, state.defaultSrcAmount)
          .accountsPartial(state.buildAccountsDataForFill({}))
          .signers([state.bob.keypair])
          .transaction()
          .then((tx) =>
            sendAndConfirmTransaction(provider.connection, tx, [
              state.alice.keypair,
            ])
          )
      ).to.be.rejectedWith("Error Code: ConstraintSeeds");
    });

    it.skip("Doesn't execute the trade with the wrong escrow ata", async () => {
      await expect(
        program.methods
          .fill(state.escrows[0].orderConfig.id, state.defaultSrcAmount)
          .accountsPartial(
            state.buildAccountsDataForFill({
              escrowSrcAta: state.escrows[1].ata,
            })
          )
          .signers([state.bob.keypair])
          .transaction()
          .then((tx) =>
            sendAndConfirmTransaction(provider.connection, tx, [
              state.bob.keypair,
            ])
          )
      ).to.be.rejectedWith("Error Code: ConstraintTokenOwner");
    });

    it.skip("Doesn't execute the trade with the wrong dstMint", async () => {
      await expect(
        program.methods
          .fill(state.escrows[0].orderConfig.id, state.defaultSrcAmount)
          .accountsPartial(
            state.buildAccountsDataForFill({
              dstMint: state.tokens[0],
            })
          )
          .signers([state.bob.keypair])
          .transaction()
          .then((tx) =>
            sendAndConfirmTransaction(provider.connection, tx, [
              state.bob.keypair,
            ])
          )
      ).to.be.rejectedWith("Error Code: ConstraintTokenMint");
    });

    it.skip("Doesn't execute the trade with the wrong maker receiver", async () => {
      await expect(
        program.methods
          .fill(state.escrows[0].orderConfig.id, state.defaultSrcAmount)
          .accountsPartial(
            state.buildAccountsDataForFill({
              makerReceiver: state.charlie.keypair.publicKey,
              makerDstAta:
                state.charlie.atas[state.tokens[1].toString()].address,
            })
          )
          .signers([state.bob.keypair])
          .transaction()
          .then((tx) =>
            sendAndConfirmTransaction(provider.connection, tx, [
              state.bob.keypair,
            ])
          )
      ).to.be.rejectedWith("Error Code: SellerReceiverMismatch");
    });

    it("Doesn't create escrow with the wrong surplus param", async () => {
      const orderConfig = state.orderConfig({
        fee: { surplusPercentage: 146 }, // 146%
      });

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
          .create(orderConfig as ReducedOrderConfig)
          .accountsPartial({
            maker: state.alice.keypair.publicKey,
            srcMint: state.tokens[0],
            dstMint: state.tokens[1],
            protocolDstAcc: null,
            integratorDstAcc: null,
            escrow: escrow,
            srcTokenProgram: splToken.TOKEN_PROGRAM_ID,
            makerReceiver: state.alice.keypair.publicKey,
          })
          .signers([state.alice.keypair])
          .transaction()
          .then((tx) =>
            sendAndConfirmTransaction(provider.connection, tx, [
              state.alice.keypair,
            ])
          )
      ).to.be.rejectedWith(errorCodeHex("InvalidProtocolSurplusFee"));
    });

    it("Doesn't create escrow with protocol_dst_acc from different mint", async () => {
      const orderConfig = state.orderConfig({
        fee: { protocolFee: 10000 }, // 10%
      });

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
          .create(orderConfig as ReducedOrderConfig)
          .accountsPartial({
            maker: state.alice.keypair.publicKey,
            srcMint: state.tokens[0],
            dstMint: state.tokens[1],
            protocolDstAcc:
              state.charlie.atas[state.tokens[0].toString()].address,
            integratorDstAcc: null,
            escrow: escrow,
            srcTokenProgram: splToken.TOKEN_PROGRAM_ID,
            makerReceiver: state.alice.keypair.publicKey,
          })
          .signers([state.alice.keypair])
          .transaction()
          .then((tx) =>
            sendAndConfirmTransaction(provider.connection, tx, [
              state.alice.keypair,
            ])
          )
      ).to.be.rejectedWith(errorCodeHex("ConstraintSeeds"));
    });

    it("Doesn't create escrow with intergrator_dst_acc from different mint", async () => {
      const orderConfig = state.orderConfig({
        fee: { integratorFee: 10000 }, // 10%
      });

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
          .create(orderConfig as ReducedOrderConfig)
          .accountsPartial({
            maker: state.alice.keypair.publicKey,
            srcMint: state.tokens[0],
            dstMint: state.tokens[1],
            protocolDstAcc: null,
            integratorDstAcc:
              state.charlie.atas[state.tokens[0].toString()].address,
            escrow: escrow,
            srcTokenProgram: splToken.TOKEN_PROGRAM_ID,
            makerReceiver: state.alice.keypair.publicKey,
          })
          .signers([state.alice.keypair])
          .transaction()
          .then((tx) =>
            sendAndConfirmTransaction(provider.connection, tx, [
              state.alice.keypair,
            ])
          )
      ).to.be.rejectedWith(errorCodeHex("ConstraintSeeds"));
    });

    it.skip("Doesn't execute the trade with the wrong protocol_dst_acc", async () => {
      const escrow = await state.createEscrow({
        escrowProgram: program,
        payer,
        provider,
        orderConfig: state.orderConfig({
          fee: {
            protocolFee: 10000, // 10%
            protocolDstAcc:
              state.charlie.atas[state.tokens[1].toString()].address,
          },
        }),
      });

      await expect(
        program.methods
          .fill(escrow.orderConfig.id, state.defaultSrcAmount)
          .accountsPartial(
            state.buildAccountsDataForFill({
              escrow: escrow.escrow,
              escrowSrcAta: escrow.ata,
              protocolDstAcc:
                state.bob.atas[state.tokens[1].toString()].address, // wrong protocol_dst_acc
            })
          )
          .signers([state.bob.keypair])
          .transaction()
          .then((tx) =>
            sendAndConfirmTransaction(provider.connection, tx, [
              state.bob.keypair,
            ])
          )
      ).to.be.rejectedWith("Error Code: InconsistentProtocolFeeConfig");
    });

    it.skip("Doesn't execute the trade without protocol_dst_acc", async () => {
      const escrow = await state.createEscrow({
        escrowProgram: program,
        payer,
        provider,
        orderConfig: state.orderConfig({
          fee: {
            protocolFee: 10000, // 10%
            protocolDstAcc:
              state.charlie.atas[state.tokens[1].toString()].address,
          },
        }),
      });

      await expect(
        program.methods
          .fill(escrow.orderConfig.id, state.defaultSrcAmount)
          .accountsPartial(
            state.buildAccountsDataForFill({
              escrow: escrow.escrow,
              escrowSrcAta: escrow.ata,
            })
          )
          .signers([state.bob.keypair])
          .transaction()
          .then((tx) =>
            sendAndConfirmTransaction(provider.connection, tx, [
              state.bob.keypair,
            ])
          )
      ).to.be.rejectedWith("Error Code: InconsistentProtocolFeeConfig");
    });

    it.skip("Doesn't execute the trade with the wrong integrator_dst_acc", async () => {
      const escrow = await state.createEscrow({
        escrowProgram: program,
        payer,
        provider,
        orderConfig: state.orderConfig({
          fee: {
            integratorFee: 10000, // 10%
            integratorDstAcc:
              state.charlie.atas[state.tokens[1].toString()].address,
          },
        }),
      });

      await expect(
        program.methods
          .fill(escrow.orderConfig.id, state.defaultSrcAmount)
          .accountsPartial(
            state.buildAccountsDataForFill({
              escrow: escrow.escrow,
              escrowSrcAta: escrow.ata,
              integratorDstAcc:
                state.bob.atas[state.tokens[1].toString()].address, // wrong integrator_dst_acc
            })
          )
          .signers([state.bob.keypair])
          .transaction()
          .then((tx) =>
            sendAndConfirmTransaction(provider.connection, tx, [
              state.bob.keypair,
            ])
          )
      ).to.be.rejectedWith("Error Code: InconsistentIntegratorFeeConfig");
    });

    it.skip("Doesn't execute the trade without integrator_dst_acc", async () => {
      const escrow = await state.createEscrow({
        escrowProgram: program,
        payer,
        provider,
        orderConfig: state.orderConfig({
          fee: {
            integratorFee: 10000, // 10%
            integratorDstAcc:
              state.charlie.atas[state.tokens[1].toString()].address,
          },
        }),
      });

      await expect(
        program.methods
          .fill(escrow.orderConfig.id, state.defaultSrcAmount)
          .accountsPartial(
            state.buildAccountsDataForFill({
              escrow: escrow.escrow,
              escrowSrcAta: escrow.ata,
            })
          )
          .signers([state.bob.keypair])
          .transaction()
          .then((tx) =>
            sendAndConfirmTransaction(provider.connection, tx, [
              state.bob.keypair,
            ])
          )
      ).to.be.rejectedWith("Error Code: InconsistentIntegratorFeeConfig");
    });

    it.skip("Execute the multiple trades", async () => {
      let transactionPromise = () =>
        program.methods
          .fill(state.escrows[0].orderConfig.id, state.defaultSrcAmount.divn(2))
          .accountsPartial(state.buildAccountsDataForFill({}))
          .signers([state.bob.keypair])
          .transaction()
          .then((tx) =>
            sendAndConfirmTransaction(provider.connection, tx, [
              state.bob.keypair,
            ])
          );

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
          .fill(state.escrows[0].orderConfig.id, state.defaultSrcAmount.divn(2))
          .accountsPartial(state.buildAccountsDataForFill({}))
          .signers([state.bob.keypair])
          .transaction()
          .then((tx) =>
            sendAndConfirmTransaction(provider.connection, tx, [
              state.bob.keypair,
            ])
          );

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

    it.skip("Execute the multiple trades, rounding", async () => {
      const _srcAmount = new anchor.BN(101);
      const _dstAmount = new anchor.BN(101);
      const escrow = await state.createEscrow({
        escrowProgram: program,
        payer,
        provider,
        orderConfig: state.orderConfig({
          srcAmount: _srcAmount,
          srcRemaining: _srcAmount,
          minDstAmount: _dstAmount,
          estimatedDstAmount: _dstAmount,
        }),
      });

      let transactionPromise = () =>
        program.methods
          .fill(escrow.orderConfig.id, _srcAmount.divn(2))
          .accountsPartial(
            state.buildAccountsDataForFill({
              escrow: escrow.escrow,
              escrowSrcAta: escrow.ata,
            })
          )
          .signers([state.bob.keypair])
          .transaction()
          .then((tx) =>
            sendAndConfirmTransaction(provider.connection, tx, [
              state.bob.keypair,
            ])
          );

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
          .fill(escrow.orderConfig.id, _srcAmount.divn(2))
          .accountsPartial(
            state.buildAccountsDataForFill({
              escrow: escrow.escrow,
              escrowSrcAta: escrow.ata,
            })
          )
          .signers([state.bob.keypair])
          .transaction()
          .then((tx) =>
            sendAndConfirmTransaction(provider.connection, tx, [
              state.bob.keypair,
            ])
          );

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
          .fill(escrow.orderConfig.id, new anchor.BN(1))
          .accountsPartial(
            state.buildAccountsDataForFill({
              escrow: escrow.escrow,
              escrowSrcAta: escrow.ata,
            })
          )
          .signers([state.bob.keypair])
          .transaction()
          .then((tx) =>
            sendAndConfirmTransaction(provider.connection, tx, [
              state.bob.keypair,
            ])
          );

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

    it.skip("Cancel the trade", async () => {
      const transactionPromise = () =>
        program.methods
          .cancel(state.escrows[0].orderConfig.id)
          .accountsPartial({
            maker: state.alice.keypair.publicKey,
            srcMint: state.tokens[0],
            escrow: state.escrows[0].escrow,
            srcTokenProgram: splToken.TOKEN_PROGRAM_ID,
          })
          .signers([state.alice.keypair])
          .transaction()
          .then((tx) =>
            sendAndConfirmTransaction(provider.connection, tx, [
              state.alice.keypair,
            ])
          );

      const results = await trackReceivedTokenAndTx(
        provider.connection,
        [state.alice.atas[state.tokens[0].toString()].address],
        transactionPromise
      );

      expect(results).to.be.deep.eq([
        BigInt(state.defaultSrcAmount.toNumber()),
      ]);
    });

    it.skip("Cancel the trade with native tokens", async () => {
      const escrow = await state.createEscrow({
        escrowProgram: program,
        payer,
        provider,
        orderConfig: {
          srcMint: splToken.NATIVE_MINT,
        },
      });

      const transactionPromise = () =>
        program.methods
          .cancel(escrow.orderConfig.id)
          .accountsPartial({
            maker: state.alice.keypair.publicKey,
            srcMint: splToken.NATIVE_MINT,
            escrow: escrow.escrow,
            srcTokenProgram: splToken.TOKEN_PROGRAM_ID,
          })
          .signers([state.alice.keypair])
          .transaction()
          .then((tx) =>
            sendAndConfirmTransaction(provider.connection, tx, [
              state.alice.keypair,
            ])
          );

      const results = await trackReceivedTokenAndTx(
        provider.connection,
        [state.alice.atas[splToken.NATIVE_MINT.toString()].address],
        transactionPromise
      );

      expect(results).to.be.deep.eq([
        BigInt(state.defaultSrcAmount.toNumber()),
      ]);
    });

    it.skip("Doesn't cancel the trade with the wrong order_id", async () => {
      await expect(
        program.methods
          .cancel(state.escrows[1].orderConfig.id)
          .accountsPartial({
            maker: state.alice.keypair.publicKey,
            srcMint: state.tokens[0],
            escrow: state.escrows[0].escrow,
            srcTokenProgram: splToken.TOKEN_PROGRAM_ID,
          })
          .signers([state.alice.keypair])
          .transaction()
          .then((tx) =>
            sendAndConfirmTransaction(provider.connection, tx, [
              state.alice.keypair,
            ])
          )
      ).to.be.rejectedWith("Error Code: ConstraintSeeds");
    });

    it.skip("Doesn't cancel the trade with the wrong escrow ata", async () => {
      await expect(
        program.methods
          .cancel(state.escrows[0].orderConfig.id)
          .accountsPartial({
            maker: state.alice.keypair.publicKey,
            srcMint: state.tokens[0],
            escrow: state.escrows[0].escrow,
            escrowSrcAta: state.escrows[1].ata,
            srcTokenProgram: splToken.TOKEN_PROGRAM_ID,
          })
          .signers([state.alice.keypair])
          .transaction()
          .then((tx) =>
            sendAndConfirmTransaction(provider.connection, tx, [
              state.alice.keypair,
            ])
          )
      ).to.be.rejectedWith("Error Code: ConstraintTokenOwner");
    });

    it.skip("Doesn't cancel the trade with the wrong maker", async () => {
      await expect(
        program.methods
          .cancel(state.escrows[0].orderConfig.id)
          .accountsPartial({
            maker: state.charlie.keypair.publicKey,
            srcMint: state.tokens[0],
            escrow: state.escrows[0].escrow,
            srcTokenProgram: splToken.TOKEN_PROGRAM_ID,
          })
          .signers([state.charlie.keypair])
          .transaction()
          .then((tx) =>
            sendAndConfirmTransaction(provider.connection, tx, [
              state.charlie.keypair,
            ])
          )
      ).to.be.rejectedWith("Error Code: ConstraintSeeds");
    });

    it.skip("Fails when taker isn't whitelisted", async () => {
      const escrow = await state.createEscrow({
        escrowProgram: program,
        payer,
        provider,
      });

      await expect(
        program.methods
          .fill(escrow.orderConfig.id, state.defaultSrcAmount.divn(2))
          .accountsPartial(
            state.buildAccountsDataForFill({
              escrow: escrow.escrow,
              escrowSrcAta: escrow.ata,
              taker: state.charlie.keypair.publicKey,
              takerDstAta:
                state.charlie.atas[state.tokens[1].toString()].address,
              takerSrcAta:
                state.charlie.atas[state.tokens[0].toString()].address,
            })
          )
          .signers([state.charlie.keypair])
          .transaction()
          .then((tx) =>
            sendAndConfirmTransaction(provider.connection, tx, [
              state.charlie.keypair,
            ])
          )
      ).to.be.rejectedWith(
        "AnchorError caused by account: resolver_access. Error Code: AccountNotInitialized"
      );
    });

    it.skip("Execute the partial fill and close escow after", async () => {
      const escrow = await state.createEscrow({
        escrowProgram: program,
        payer,
        provider,
      });

      // Fill the trade partially
      const transactionPromiseFill = () =>
        program.methods
          .fill(escrow.orderConfig.id, state.defaultSrcAmount.divn(2))
          .accountsPartial(
            state.buildAccountsDataForFill({
              escrow: escrow.escrow,
              escrowSrcAta: escrow.ata,
            })
          )
          .signers([state.bob.keypair])
          .transaction()
          .then((tx) =>
            sendAndConfirmTransaction(provider.connection, tx, [
              state.bob.keypair,
            ])
          );

      const resultsFill = await trackReceivedTokenAndTx(
        provider.connection,
        [
          escrow.ata,
          state.alice.atas[state.tokens[1].toString()].address,
          state.bob.atas[state.tokens[0].toString()].address,
          state.bob.atas[state.tokens[1].toString()].address,
        ],
        transactionPromiseFill
      );

      expect(resultsFill).to.be.deep.eq([
        -BigInt(state.defaultSrcAmount.divn(2).toNumber()),
        BigInt(state.defaultDstAmount.divn(2).toNumber()),
        BigInt(state.defaultSrcAmount.divn(2).toNumber()),
        -BigInt(state.defaultDstAmount.divn(2).toNumber()),
      ]);

      // Cancel the trade
      const transactionPromiseCancel = () =>
        program.methods
          .cancel(escrow.orderConfig.id)
          .accountsPartial({
            maker: state.alice.keypair.publicKey,
            srcMint: state.tokens[0],
            escrow: escrow.escrow,
            srcTokenProgram: splToken.TOKEN_PROGRAM_ID,
          })
          .signers([state.alice.keypair])
          .transaction()
          .then((tx) =>
            sendAndConfirmTransaction(provider.connection, tx, [
              state.alice.keypair,
            ])
          );

      const resultsCancel = await trackReceivedTokenAndTx(
        provider.connection,
        [state.alice.atas[state.tokens[0].toString()].address],
        transactionPromiseCancel
      );

      expect(resultsCancel).to.be.deep.eq([
        BigInt(state.defaultSrcAmount.divn(2).toNumber()),
      ]);
    });

    it.skip("Execute the trade with native tokens (SOL) as destination", async () => {
      const makerNativeTokenBalanceBefore =
        await provider.connection.getBalance(state.alice.keypair.publicKey);

      const escrow = await state.createEscrow({
        escrowProgram: program,
        payer,
        provider,
        orderConfig: state.orderConfig({
          dstMint: splToken.NATIVE_MINT,
          nativeDstAsset: true,
        }),
      });

      await program.methods
        .fill(escrow.orderConfig.id, state.defaultSrcAmount)
        .accountsPartial(
          state.buildAccountsDataForFill({
            escrow: escrow.escrow,
            escrowSrcAta: escrow.ata,
            dstMint: splToken.NATIVE_MINT,
            makerDstAta: null,
            takerDstAta:
              state.bob.atas[splToken.NATIVE_MINT.toString()].address,
          })
        )
        .signers([state.bob.keypair])
        .transaction()
        .then((tx) =>
          sendAndConfirmTransaction(provider.connection, tx, [
            state.bob.keypair,
          ])
        );

      const makerNativeTokenBalanceAfter = await provider.connection.getBalance(
        state.alice.keypair.publicKey
      );

      // check that native tokens were sent to maker
      expect(makerNativeTokenBalanceAfter).to.be.eq(
        makerNativeTokenBalanceBefore + state.defaultDstAmount.toNumber()
      );

      // Verify that the escrow account was closed
      await expect(
        splToken.getAccount(provider.connection, escrow.ata)
      ).to.be.rejectedWith(splToken.TokenAccountNotFoundError);
    });

    it.skip("Fails to execute the trade if maker_dst_ata is missing", async () => {
      const escrow = await state.createEscrow({
        escrowProgram: program,
        payer,
        provider,
        orderConfig: {
          dstMint: splToken.NATIVE_MINT,
        },
      });

      await expect(
        program.methods
          .fill(escrow.orderConfig.id, state.defaultSrcAmount)
          .accounts(
            state.buildAccountsDataForFill({
              escrow: escrow.escrow,
              escrowSrcAta: escrow.ata,
              dstMint: splToken.NATIVE_MINT,
              makerDstAta: null,
              takerDstAta:
                state.bob.atas[splToken.NATIVE_MINT.toString()].address,
            })
          )
          .signers([state.bob.keypair])
          .transaction()
          .then((tx) =>
            sendAndConfirmTransaction(provider.connection, tx, [
              state.bob.keypair,
            ])
          )
      ).to.be.rejectedWith("Error Code: MissingMakerDstAta");
    });

    it("Fails to create if native_dst_asset = true but mint is different from native mint", async () => {
      const orderConfig = state.orderConfig({
        nativeDstAsset: true,
      });

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
          .create(orderConfig as ReducedOrderConfig)
          .accountsPartial({
            maker: state.alice.keypair.publicKey,
            srcMint: state.tokens[0],
            dstMint: state.tokens[1],
            protocolDstAcc: null,
            integratorDstAcc: null,
            escrow: escrow,
            srcTokenProgram: splToken.TOKEN_PROGRAM_ID,
            makerReceiver: state.alice.keypair.publicKey,
          })
          .signers([state.alice.keypair])
          .transaction()
          .then((tx) =>
            sendAndConfirmTransaction(provider.connection, tx, [
              state.alice.keypair,
            ])
          )
      ).to.be.rejectedWith(errorCodeHex("InconsistentNativeDstTrait"));
    });

    it.skip("Execute the trade and transfer wSOL if native_dst_asset = false and native dst mint is provided", async () => {
      const escrow = await state.createEscrow({
        escrowProgram: program,
        payer,
        provider,
        orderConfig: state.orderConfig({
          dstMint: splToken.NATIVE_MINT,
          nativeDstAsset: false,
        }),
      });

      const transactionPromise = () =>
        program.methods
          .fill(escrow.orderConfig.id, state.defaultSrcAmount)
          .accountsPartial(
            state.buildAccountsDataForFill({
              escrow: escrow.escrow,
              escrowSrcAta: escrow.ata,
              dstMint: splToken.NATIVE_MINT,
              makerDstAta:
                state.alice.atas[splToken.NATIVE_MINT.toString()].address,
              takerDstAta:
                state.bob.atas[splToken.NATIVE_MINT.toString()].address,
            })
          )
          .signers([state.bob.keypair])
          .transaction()
          .then((tx) =>
            sendAndConfirmTransaction(provider.connection, tx, [
              state.bob.keypair,
            ])
          );

      const results = await trackReceivedTokenAndTx(
        provider.connection,
        [
          state.alice.atas[splToken.NATIVE_MINT.toString()].address,
          state.bob.atas[state.tokens[0].toString()].address,
          state.bob.atas[splToken.NATIVE_MINT.toString()].address,
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
  });

  describe.skip("Optional tests", () => {
    it("Doesn't execute the trade with the wrong maker's ata", async () => {
      await expect(
        program.methods
          .fill(state.escrows[0].orderConfig.id, state.defaultSrcAmount)
          .accountsPartial(
            state.buildAccountsDataForFill({
              makerDstAta:
                state.charlie.atas[state.tokens[1].toString()].address,
            })
          )
          .signers([state.bob.keypair])
          .transaction()
          .then((tx) =>
            sendAndConfirmTransaction(provider.connection, tx, [
              state.bob.keypair,
            ])
          )
      ).to.be.rejectedWith("Error Code: ConstraintTokenOwner");
    });

    it("Doesn't execute the trade with the wrong token", async () => {
      await expect(
        program.methods
          .fill(state.escrows[0].orderConfig.id, state.defaultSrcAmount)
          .accountsPartial(
            state.buildAccountsDataForFill({
              srcMint: state.tokens[1],
            })
          )
          .signers([state.bob.keypair])
          .transaction()
          .then((tx) =>
            sendAndConfirmTransaction(provider.connection, tx, [
              state.bob.keypair,
            ])
          )
      ).to.be.rejectedWith("Error Code: ConstraintAssociated");
    });
  });

  describe.skip("Multiple escrows", () => {
    it("Double fill", async () => {
      const transactionPromise = async () => {
        await program.methods
          .fill(state.escrows[0].orderConfig.id, state.defaultSrcAmount)
          .accountsPartial(state.buildAccountsDataForFill({}))
          .signers([state.bob.keypair])
          .transaction()
          .then((tx) =>
            sendAndConfirmTransaction(provider.connection, tx, [
              state.bob.keypair,
            ])
          );

        // Add Charlie to the whitelist
        await createWhitelistedAccount(
          whitelistProgram,
          state.charlie.keypair,
          payer
        );
        await program.methods
          .fill(state.escrows[1].orderConfig.id, state.defaultSrcAmount)
          .accountsPartial(
            state.buildAccountsDataForFill({
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
          .transaction()
          .then((tx) =>
            sendAndConfirmTransaction(provider.connection, tx, [
              state.charlie.keypair,
            ])
          );

        // Remove Charlie from the whitelist
        await removeWhitelistedAccount(state.charlie.keypair, payer);
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
  });
});
