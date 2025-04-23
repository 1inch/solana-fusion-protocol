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
  let newAuthority: anchor.web3.Keypair;
  let whitelistPDA: anchor.web3.PublicKey;

  before(async () => {
    userToWhitelist = anchor.web3.Keypair.generate();
    await provider.connection.requestAirdrop(
      userToWhitelist.publicKey,
      1 * LAMPORTS_PER_SOL
    );

    // Generate new authority keypair and fund it
    newAuthority = anchor.web3.Keypair.generate();
    await provider.connection.requestAirdrop(
      newAuthority.publicKey,
      1 * LAMPORTS_PER_SOL
    );

    [whitelistPDA] = anchor.web3.PublicKey.findProgramAddressSync(
      [Buffer.from("resolver_access"), userToWhitelist.publicKey.toBuffer()],
      program.programId
    );

    // Initialize the whitelist state with the payer as authority
    await initializeWhitelist(program, payer);
  });

  it("Can register and deregister a user from whitelist", async () => {
    // Register the user
    await program.methods
      .register(userToWhitelist.publicKey)
      .accountsPartial({
        authority: payer.publicKey,
      })
      .signers([payer])
      .rpc();

    // Verify the whitelist account exists
    const whitelistAccount = await program.account.resolverAccess.fetch(
      whitelistPDA
    );
    expect(whitelistAccount).to.not.be.null;

    // Deregister the user
    await program.methods
      .deregister(userToWhitelist.publicKey)
      .accountsPartial({
        authority: payer.publicKey,
      })
      .signers([payer])
      .rpc();

    // Verify the whitelist account does not exist
    await expect(
      program.account.resolverAccess.fetch(whitelistPDA)
    ).to.be.rejectedWith("Account does not exist");
  });

  it("Stores the canonical bump in the whitelist account", async () => {
    // Get the canonical bump
    const [, canonicalBump] = anchor.web3.PublicKey.findProgramAddressSync(
      [Buffer.from("resolver_access"), userToWhitelist.publicKey.toBuffer()],
      program.programId
    );

    // Register the user
    await program.methods
      .register(userToWhitelist.publicKey)
      .accountsPartial({
        authority: payer.publicKey,
      })
      .signers([payer])
      .rpc();

    // Verify the whitelist account stores the canonical bump
    const whitelistAccount = await program.account.resolverAccess.fetch(
      whitelistPDA
    );
    expect(whitelistAccount.bump).to.be.equal(canonicalBump);

    // Deregister the user to not interfere with other tests
    await program.methods
      .deregister(userToWhitelist.publicKey)
      .accountsPartial({
        authority: payer.publicKey,
      })
      .signers([payer])
      .rpc();
  });

  it("Cannot register the same user twice", async () => {
    // First registration
    await program.methods
      .register(userToWhitelist.publicKey)
      .accountsPartial({
        authority: payer.publicKey,
      })
      .signers([payer])
      .rpc();

    // Second registration should fail
    await expect(
      program.methods
        .register(userToWhitelist.publicKey)
        .accountsPartial({
          authority: payer.publicKey,
        })
        .signers([payer])
        .rpc()
    ).to.be.rejected;

    // Cleanup
    await program.methods
      .deregister(userToWhitelist.publicKey)
      .accountsPartial({
        authority: payer.publicKey,
      })
      .signers([payer])
      .rpc();
  });

  it("Can set authority to new account", async () => {
    // Set new authority
    await program.methods
      .setAuthority(newAuthority.publicKey)
      .accountsPartial({
        currentAuthority: payer.publicKey,
      })
      .signers([payer])
      .rpc();

    // Verify the new authority is set correctly
    const [whitelistStatePDA] = anchor.web3.PublicKey.findProgramAddressSync(
      [Buffer.from("whitelist_state")],
      program.programId
    );
    const whitelistState = await program.account.whitelistState.fetch(
      whitelistStatePDA
    );
    expect(whitelistState.authority.toString()).to.equal(
      newAuthority.publicKey.toString()
    );
  });

  it("New authority can register and deregister users", async () => {
    // New authority should be able to register a user
    await program.methods
      .register(userToWhitelist.publicKey)
      .accountsPartial({
        authority: newAuthority.publicKey,
      })
      .signers([newAuthority])
      .rpc();

    // Verify the whitelist account exists
    const whitelistAccount = await program.account.resolverAccess.fetch(
      whitelistPDA
    );
    expect(whitelistAccount).to.not.be.null;

    // New authority should be able to deregister the user
    await program.methods
      .deregister(userToWhitelist.publicKey)
      .accountsPartial({
        authority: newAuthority.publicKey,
      })
      .signers([newAuthority])
      .rpc();

    // Verify the whitelist account does not exist
    await expect(
      program.account.resolverAccess.fetch(whitelistPDA)
    ).to.be.rejectedWith("Account does not exist");
  });

  it("Cannot register with wrong authority", async () => {
    await expect(
      program.methods
        .register(userToWhitelist.publicKey)
        .accountsPartial({
          authority: userToWhitelist.publicKey,
        })
        .signers([userToWhitelist])
        .rpc()
    ).to.be.rejectedWith("Error Code: Unauthorized");
  });

  it("Cannot deregister with wrong authority", async () => {
    // First register the user
    await program.methods
      .register(userToWhitelist.publicKey)
      .accountsPartial({
        authority: newAuthority.publicKey,
      })
      .signers([newAuthority])
      .rpc();

    // Try to deregister with wrong authority
    await expect(
      program.methods
        .deregister(userToWhitelist.publicKey)
        .accountsPartial({
          authority: userToWhitelist.publicKey,
        })
        .signers([userToWhitelist])
        .rpc()
    ).to.be.rejectedWith("Error Code: Unauthorized");

    // Cleanup
    await program.methods
      .deregister(userToWhitelist.publicKey)
      .accountsPartial({
        authority: newAuthority.publicKey,
      })
      .signers([newAuthority])
      .rpc();
  });

  it("Previous authority cannot register or deregister users", async () => {
    // Previous authority should not be able to register a user
    await expect(
      program.methods
        .register(userToWhitelist.publicKey)
        .accountsPartial({
          authority: payer.publicKey,
        })
        .signers([payer])
        .rpc()
    ).to.be.rejectedWith("Error Code: Unauthorized");

    // Register user with new authority for deregister test
    await program.methods
      .register(userToWhitelist.publicKey)
      .accountsPartial({
        authority: newAuthority.publicKey,
      })
      .signers([newAuthority])
      .rpc();

    // Previous authority should not be able to deregister a user
    await expect(
      program.methods
        .deregister(userToWhitelist.publicKey)
        .accountsPartial({
          authority: payer.publicKey,
        })
        .signers([payer])
        .rpc()
    ).to.be.rejectedWith("Error Code: Unauthorized");
  });

  it("Non-authority cannot set authority", async () => {
    const randomUser = anchor.web3.Keypair.generate();
    await provider.connection.requestAirdrop(
      randomUser.publicKey,
      1 * LAMPORTS_PER_SOL
    );

    await expect(
      program.methods
        .setAuthority(newAuthority.publicKey)
        .accountsPartial({
          currentAuthority: randomUser.publicKey,
        })
        .signers([randomUser])
        .rpc()
    ).to.be.rejectedWith("Error Code: Unauthorized");
  });
});
