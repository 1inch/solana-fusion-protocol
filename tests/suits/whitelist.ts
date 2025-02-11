import * as anchor from "@coral-xyz/anchor";
import NodeWallet from "@coral-xyz/anchor/dist/cjs/nodewallet";
import { LAMPORTS_PER_SOL } from "@solana/web3.js";
import chai, { expect } from "chai";
import chaiAsPromised from "chai-as-promised";
import { debugLog } from "../utils/utils";
import { Whitelist } from "../../target/types/whitelist";

chai.use(chaiAsPromised);

describe.only("Whitelist", () => {
  const provider = anchor.AnchorProvider.env();
  anchor.setProvider(provider);

  const program = anchor.workspace.Whitelist as anchor.Program<Whitelist>;
  const payer = (provider.wallet as NodeWallet).payer;
  debugLog(`Payer ::`, payer.publicKey.toString());

  let userToWhitelist: anchor.web3.Keypair;
  let whitelistPDA: anchor.web3.PublicKey;

  before(async () => {
    userToWhitelist = anchor.web3.Keypair.generate();
    await provider.connection.requestAirdrop(
      userToWhitelist.publicKey,
      1 * LAMPORTS_PER_SOL
    );

    [whitelistPDA] = anchor.web3.PublicKey.findProgramAddressSync(
      [Buffer.from("whitelist"), userToWhitelist.publicKey.toBuffer()],
      program.programId
    );

    // Initialize the whitelist state with the payer as owner
    await program.methods
      .initialize()
      .accountsPartial({
        owner: payer.publicKey,
      })
      .signers([payer])
      .rpc();
  });

  it("Can register and deregister a user from whitelist", async () => {
    // Register the user
    await program.methods
      .register()
      .accountsPartial({
        owner: payer.publicKey,
        user: userToWhitelist.publicKey,
      })
      .signers([payer])
      .rpc();

    // Verify the whitelist account exists
    const whitelistAccount = await program.account.whitelisted.fetch(
      whitelistPDA
    );
    expect(whitelistAccount).to.not.be.null;

    // Deregister the user
    await program.methods
      .deregister()
      .accountsPartial({
        owner: payer.publicKey,
        user: userToWhitelist.publicKey,
      })
      .signers([payer])
      .rpc();

    // Verify the whitelist account does not exist
      await expect(
        program.account.whitelisted.fetch(whitelistPDA)
      ).to.be.rejectedWith("Account does not exist");
  });

  describe("Error cases", () => {
    it("Cannot register with wrong owner", async () => {
      await expect(
        program.methods
          .register()
          .accountsPartial({
            owner: userToWhitelist.publicKey,
            user: userToWhitelist.publicKey,
          })
          .signers([userToWhitelist])
          .rpc()
      ).to.be.rejectedWith("Error Code: UnauthorizedOwner");
    });

    it("Cannot register the same user twice", async () => {
      // First registration
      await program.methods
        .register()
        .accountsPartial({
          owner: payer.publicKey,
          user: userToWhitelist.publicKey,
        })
        .signers([payer])
        .rpc();

      // Second registration should fail
      await expect(
        program.methods
          .register()
          .accountsPartial({
            owner: payer.publicKey,
            user: userToWhitelist.publicKey,
          })
          .signers([payer])
          .rpc()
      ).to.be.rejected;

      // Cleanup
      await program.methods
        .deregister()
        .accountsPartial({
          owner: payer.publicKey,
          user: userToWhitelist.publicKey,
        })
        .signers([payer])
        .rpc();
    });

    it("Cannot deregister with wrong owner", async () => {
      // First register the user
      await program.methods
        .register()
        .accountsPartial({
          owner: payer.publicKey,
          user: userToWhitelist.publicKey,
        })
        .signers([payer])
        .rpc();

      // Try to deregister with wrong owner
      await expect(
        program.methods
          .deregister()
          .accountsPartial({
            owner: userToWhitelist.publicKey,
            user: userToWhitelist.publicKey,
          })
          .signers([userToWhitelist])
          .rpc()
      ).to.be.rejectedWith("Error Code: UnauthorizedOwner");

      // Cleanup
      await program.methods
        .deregister()
        .accountsPartial({
          owner: payer.publicKey,
          user: userToWhitelist.publicKey,
        })
        .signers([payer])
        .rpc();
    });
  });
});
