import * as anchor from "@coral-xyz/anchor";
import NodeWallet from "@coral-xyz/anchor/dist/cjs/nodewallet";
import { LAMPORTS_PER_SOL } from "@solana/web3.js";
import chai, { expect } from "chai";
import chaiAsPromised from "chai-as-promised";
import { debugLog, initializeWhitelist } from "../utils/utils";
import { Whitelist } from "../../target/types/whitelist";

chai.use(chaiAsPromised);

describe("Whitelist", () => {
  const provider = anchor.AnchorProvider.env();
  anchor.setProvider(provider);

  const program = anchor.workspace.Whitelist as anchor.Program<Whitelist>;
  const payer = (provider.wallet as NodeWallet).payer;
  debugLog(`Payer ::`, payer.publicKey.toString());

  let userToWhitelist: anchor.web3.Keypair;
  let newOwner: anchor.web3.Keypair;
  let whitelistPDA: anchor.web3.PublicKey;

  before(async () => {
    userToWhitelist = anchor.web3.Keypair.generate();
    await provider.connection.requestAirdrop(
      userToWhitelist.publicKey,
      1 * LAMPORTS_PER_SOL
    );

    // Generate new owner keypair and fund it
    newOwner = anchor.web3.Keypair.generate();
    await provider.connection.requestAirdrop(
      newOwner.publicKey,
      1 * LAMPORTS_PER_SOL
    );

    [whitelistPDA] = anchor.web3.PublicKey.findProgramAddressSync(
      [Buffer.from("authorization"), userToWhitelist.publicKey.toBuffer()],
      program.programId
    );

    // Initialize the whitelist state with the payer as owner
    await initializeWhitelist(program, payer);
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
    const whitelistAccount = await program.account.authorization.fetch(
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
      program.account.authorization.fetch(whitelistPDA)
    ).to.be.rejectedWith("Account does not exist");
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

  it("Can transfer ownership to new owner", async () => {
    // Transfer ownership
    await program.methods
      .transferOwnership()
      .accountsPartial({
        currentOwner: payer.publicKey,
        newOwner: newOwner.publicKey,
      })
      .signers([payer])
      .rpc();

    // Verify the new owner is set correctly
    const [whitelistStatePDA] = anchor.web3.PublicKey.findProgramAddressSync(
      [Buffer.from("whitelist_state")],
      program.programId
    );
    const whitelistState = await program.account.whitelistState.fetch(
      whitelistStatePDA
    );
    expect(whitelistState.owner.toString()).to.equal(
      newOwner.publicKey.toString()
    );
  });

  it("New owner can register and deregister users", async () => {
    // New owner should be able to register a user
    await program.methods
      .register()
      .accountsPartial({
        owner: newOwner.publicKey,
        user: userToWhitelist.publicKey,
      })
      .signers([newOwner])
      .rpc();

    // Verify the whitelist account exists
    const whitelistAccount = await program.account.authorization.fetch(
      whitelistPDA
    );
    expect(whitelistAccount).to.not.be.null;

    // New owner should be able to deregister the user
    await program.methods
      .deregister()
      .accountsPartial({
        owner: newOwner.publicKey,
        user: userToWhitelist.publicKey,
      })
      .signers([newOwner])
      .rpc();

    // Verify the whitelist account does not exist
    await expect(
      program.account.authorization.fetch(whitelistPDA)
    ).to.be.rejectedWith("Account does not exist");
  });

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

  it("Cannot deregister with wrong owner", async () => {
    // First register the user
    await program.methods
      .register()
      .accountsPartial({
        owner: newOwner.publicKey,
        user: userToWhitelist.publicKey,
      })
      .signers([newOwner])
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
        owner: newOwner.publicKey,
        user: userToWhitelist.publicKey,
      })
      .signers([newOwner])
      .rpc();
  });

  it("Previous owner cannot register or deregister users", async () => {
    // Previous owner should not be able to register a user
    await expect(
      program.methods
        .register()
        .accountsPartial({
          owner: payer.publicKey,
          user: userToWhitelist.publicKey,
        })
        .signers([payer])
        .rpc()
    ).to.be.rejectedWith("Error Code: UnauthorizedOwner");

    // Register user with new owner for deregister test
    await program.methods
      .register()
      .accountsPartial({
        owner: newOwner.publicKey,
        user: userToWhitelist.publicKey,
      })
      .signers([newOwner])
      .rpc();

    // Previous owner should not be able to deregister a user
    await expect(
      program.methods
        .deregister()
        .accountsPartial({
          owner: payer.publicKey,
          user: userToWhitelist.publicKey,
        })
        .signers([payer])
        .rpc()
    ).to.be.rejectedWith("Error Code: UnauthorizedOwner");
  });

  it("Non-owner cannot transfer ownership", async () => {
    const randomUser = anchor.web3.Keypair.generate();
    await provider.connection.requestAirdrop(
      randomUser.publicKey,
      1 * LAMPORTS_PER_SOL
    );

    await expect(
      program.methods
        .transferOwnership()
        .accountsPartial({
          currentOwner: randomUser.publicKey,
          newOwner: newOwner.publicKey,
        })
        .signers([randomUser])
        .rpc()
    ).to.be.rejectedWith("Error Code: UnauthorizedOwner");
  });
});
