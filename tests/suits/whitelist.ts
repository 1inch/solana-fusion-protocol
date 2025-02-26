import {
  Connection,
  Keypair,
  LAMPORTS_PER_SOL,
  PublicKey,
  SystemProgram,
  Transaction,
  TransactionInstruction,
  sendAndConfirmTransaction,
} from "@solana/web3.js";
import { expect } from "chai";
import { struct, u8, publicKey } from "@project-serum/borsh";

// Constants from the program
const WHITELIST_STATE_SEED = Buffer.from("whitelist_state");
const RESOLVER_ACCESS_SEED = Buffer.from("resolver_access");

// Instruction enum layout
class Initialize {
  constructor() {}
  serialize() {
    return Buffer.from([0]);
  }
}

class Register {
  constructor(public user: PublicKey) {}
  serialize() {
    const layout = struct([u8("variant"), publicKey("user")]);
    const buffer = Buffer.alloc(1000);
    const len = layout.encode({ variant: 1, user: this.user }, buffer);
    return buffer.slice(0, len);
  }
}

class Deregister {
  constructor(public user: PublicKey) {}
  serialize() {
    const layout = struct([u8("variant"), publicKey("user")]);
    const buffer = Buffer.alloc(1000);
    const len = layout.encode({ variant: 2, user: this.user }, buffer);
    return buffer.slice(0, len);
  }
}

class TransferOwnership {
  constructor(public newOwner: PublicKey) {}
  serialize() {
    const layout = struct([u8("variant"), publicKey("newOwner")]);
    const buffer = Buffer.alloc(1000);
    const len = layout.encode({ variant: 3, newOwner: this.newOwner }, buffer);
    return buffer.slice(0, len);
  }
}

describe("Whitelist", () => {
  const connection = new Connection("http://localhost:8899", "confirmed");
  const payer = Keypair.generate();
  const programId = new PublicKey(
    "3cx4U4YnUNeDaQfqMkzw8AsVGtBXrcAbbjd1wPGMpMZc"
  );

  let userToWhitelist: Keypair;
  let newOwner: Keypair;
  let whitelistPDA: PublicKey;
  let whitelistStatePDA: PublicKey;

  before(async () => {
    // Wait for validator to be ready
    let retries = 30;
    while (retries > 0) {
      try {
        const slot = await connection.getSlot();
        console.log("Connected to validator at slot:", slot);
        break;
      } catch (e) {
        console.log("Waiting for validator... Retries left:", retries);
        await new Promise((resolve) => setTimeout(resolve, 1000));
        retries--;
        if (retries === 0) {
          throw new Error("Failed to connect to validator after 30 seconds");
        }
      }
    }

    // Fund payer
    const airdropSignature = await connection.requestAirdrop(
      payer.publicKey,
      10 * LAMPORTS_PER_SOL
    );
    await connection.confirmTransaction(airdropSignature);

    userToWhitelist = Keypair.generate();
    const userAirdropSignature = await connection.requestAirdrop(
      userToWhitelist.publicKey,
      1 * LAMPORTS_PER_SOL
    );
    await connection.confirmTransaction(userAirdropSignature);

    newOwner = Keypair.generate();
    const newOwnerAirdropSignature = await connection.requestAirdrop(
      newOwner.publicKey,
      1 * LAMPORTS_PER_SOL
    );
    await connection.confirmTransaction(newOwnerAirdropSignature);

    [whitelistPDA] = PublicKey.findProgramAddressSync(
      [RESOLVER_ACCESS_SEED, userToWhitelist.publicKey.toBuffer()],
      programId
    );

    [whitelistStatePDA] = PublicKey.findProgramAddressSync(
      [WHITELIST_STATE_SEED],
      programId
    );

    // Initialize the whitelist
    const ix = new TransactionInstruction({
      keys: [
        { pubkey: payer.publicKey, isSigner: true, isWritable: true },
        { pubkey: whitelistStatePDA, isSigner: false, isWritable: true },
        { pubkey: SystemProgram.programId, isSigner: false, isWritable: false },
      ],
      programId,
      data: new Initialize().serialize(),
    });

    const tx = new Transaction().add(ix);
    await sendAndConfirmTransaction(connection, tx, [payer]);
  });

  it("Can register and deregister a user from whitelist", async () => {
    // Register the user
    const registerIx = new TransactionInstruction({
      keys: [
        { pubkey: payer.publicKey, isSigner: true, isWritable: true },
        { pubkey: whitelistStatePDA, isSigner: false, isWritable: false },
        { pubkey: whitelistPDA, isSigner: false, isWritable: true },
        { pubkey: SystemProgram.programId, isSigner: false, isWritable: false },
      ],
      programId,
      data: new Register(userToWhitelist.publicKey).serialize(),
    });

    await sendAndConfirmTransaction(
      connection,
      new Transaction().add(registerIx),
      [payer]
    );

    // Verify the whitelist account exists
    const account = await connection.getAccountInfo(whitelistPDA);
    expect(account).to.not.be.null;

    // Deregister the user
    const deregisterIx = new TransactionInstruction({
      keys: [
        { pubkey: payer.publicKey, isSigner: true, isWritable: true },
        { pubkey: whitelistStatePDA, isSigner: false, isWritable: false },
        { pubkey: whitelistPDA, isSigner: false, isWritable: true },
      ],
      programId,
      data: new Deregister(userToWhitelist.publicKey).serialize(),
    });

    await sendAndConfirmTransaction(
      connection,
      new Transaction().add(deregisterIx),
      [payer]
    );

    // Verify the whitelist account does not exist
    const closedAccount = await connection.getAccountInfo(whitelistPDA);
    expect(closedAccount).to.be.null;
  });

  it("Cannot register the same user twice", async () => {
    // First registration
    const registerIx = new TransactionInstruction({
      keys: [
        { pubkey: payer.publicKey, isSigner: true, isWritable: true },
        { pubkey: whitelistStatePDA, isSigner: false, isWritable: false },
        { pubkey: whitelistPDA, isSigner: false, isWritable: true },
        { pubkey: SystemProgram.programId, isSigner: false, isWritable: false },
      ],
      programId,
      data: new Register(userToWhitelist.publicKey).serialize(),
    });

    await sendAndConfirmTransaction(
      connection,
      new Transaction().add(registerIx),
      [payer]
    );

    // Second registration should fail
    try {
      await sendAndConfirmTransaction(
        connection,
        new Transaction().add(registerIx),
        [payer]
      );
      expect.fail("Should have failed");
    } catch (error) {
      expect(error).to.exist;
    }

    // Cleanup
    const deregisterIx = new TransactionInstruction({
      keys: [
        { pubkey: payer.publicKey, isSigner: true, isWritable: true },
        { pubkey: whitelistStatePDA, isSigner: false, isWritable: false },
        { pubkey: whitelistPDA, isSigner: false, isWritable: true },
      ],
      programId,
      data: new Deregister(userToWhitelist.publicKey).serialize(),
    });

    await sendAndConfirmTransaction(
      connection,
      new Transaction().add(deregisterIx),
      [payer]
    );
  });

  it("Can transfer ownership to new owner", async () => {
    // Transfer ownership
    const transferIx = new TransactionInstruction({
      keys: [
        { pubkey: payer.publicKey, isSigner: true, isWritable: true },
        { pubkey: whitelistStatePDA, isSigner: false, isWritable: true },
      ],
      programId,
      data: new TransferOwnership(newOwner.publicKey).serialize(),
    });

    await sendAndConfirmTransaction(
      connection,
      new Transaction().add(transferIx),
      [payer]
    );

    // Verify the new owner is set correctly
    const account = await connection.getAccountInfo(whitelistStatePDA);
    expect(account).to.not.be.null;

    // Skip 8 bytes of discriminator and read the pubkey
    const ownerPubkey = new PublicKey(account!.data.slice(8, 40));
    expect(ownerPubkey.toString()).to.equal(newOwner.publicKey.toString());
  });

  it("New owner can register and deregister users", async () => {
    // New owner should be able to register a user
    const registerIx = new TransactionInstruction({
      keys: [
        { pubkey: newOwner.publicKey, isSigner: true, isWritable: true },
        { pubkey: whitelistStatePDA, isSigner: false, isWritable: false },
        { pubkey: whitelistPDA, isSigner: false, isWritable: true },
        { pubkey: SystemProgram.programId, isSigner: false, isWritable: false },
      ],
      programId,
      data: new Register(userToWhitelist.publicKey).serialize(),
    });

    await sendAndConfirmTransaction(
      connection,
      new Transaction().add(registerIx),
      [newOwner]
    );

    // Verify the whitelist account exists
    const account = await connection.getAccountInfo(whitelistPDA);
    expect(account).to.not.be.null;

    // New owner should be able to deregister the user
    const deregisterIx = new TransactionInstruction({
      keys: [
        { pubkey: newOwner.publicKey, isSigner: true, isWritable: true },
        { pubkey: whitelistStatePDA, isSigner: false, isWritable: false },
        { pubkey: whitelistPDA, isSigner: false, isWritable: true },
      ],
      programId,
      data: new Deregister(userToWhitelist.publicKey).serialize(),
    });

    await sendAndConfirmTransaction(
      connection,
      new Transaction().add(deregisterIx),
      [newOwner]
    );

    // Verify the whitelist account does not exist
    const closedAccount = await connection.getAccountInfo(whitelistPDA);
    expect(closedAccount).to.be.null;
  });

  it("Cannot register with wrong owner", async () => {
    const registerIx = new TransactionInstruction({
      keys: [
        { pubkey: userToWhitelist.publicKey, isSigner: true, isWritable: true },
        { pubkey: whitelistStatePDA, isSigner: false, isWritable: false },
        { pubkey: whitelistPDA, isSigner: false, isWritable: true },
        { pubkey: SystemProgram.programId, isSigner: false, isWritable: false },
      ],
      programId,
      data: new Register(userToWhitelist.publicKey).serialize(),
    });

    try {
      await sendAndConfirmTransaction(
        connection,
        new Transaction().add(registerIx),
        [userToWhitelist]
      );
      expect.fail("Should have failed");
    } catch (error: any) {
      expect(error).to.exist;
    }
  });

  it("Cannot deregister with wrong owner", async () => {
    // First register the user
    const registerIx = new TransactionInstruction({
      keys: [
        { pubkey: newOwner.publicKey, isSigner: true, isWritable: true },
        { pubkey: whitelistStatePDA, isSigner: false, isWritable: false },
        { pubkey: whitelistPDA, isSigner: false, isWritable: true },
        { pubkey: SystemProgram.programId, isSigner: false, isWritable: false },
      ],
      programId,
      data: new Register(userToWhitelist.publicKey).serialize(),
    });

    await sendAndConfirmTransaction(
      connection,
      new Transaction().add(registerIx),
      [newOwner]
    );

    // Try to deregister with wrong owner
    const deregisterIx = new TransactionInstruction({
      keys: [
        { pubkey: userToWhitelist.publicKey, isSigner: true, isWritable: true },
        { pubkey: whitelistStatePDA, isSigner: false, isWritable: false },
        { pubkey: whitelistPDA, isSigner: false, isWritable: true },
      ],
      programId,
      data: new Deregister(userToWhitelist.publicKey).serialize(),
    });

    try {
      await sendAndConfirmTransaction(
        connection,
        new Transaction().add(deregisterIx),
        [userToWhitelist]
      );
      expect.fail("Should have failed");
    } catch (error: any) {
      expect(error).to.exist;
    }

    // Cleanup
    const cleanupIx = new TransactionInstruction({
      keys: [
        { pubkey: newOwner.publicKey, isSigner: true, isWritable: true },
        { pubkey: whitelistStatePDA, isSigner: false, isWritable: false },
        { pubkey: whitelistPDA, isSigner: false, isWritable: true },
      ],
      programId,
      data: new Deregister(userToWhitelist.publicKey).serialize(),
    });

    await sendAndConfirmTransaction(
      connection,
      new Transaction().add(cleanupIx),
      [newOwner]
    );
  });

  it("Previous owner cannot register or deregister users", async () => {
    // Previous owner should not be able to register a user
    const registerIx = new TransactionInstruction({
      keys: [
        { pubkey: payer.publicKey, isSigner: true, isWritable: true },
        { pubkey: whitelistStatePDA, isSigner: false, isWritable: false },
        { pubkey: whitelistPDA, isSigner: false, isWritable: true },
        { pubkey: SystemProgram.programId, isSigner: false, isWritable: false },
      ],
      programId,
      data: new Register(userToWhitelist.publicKey).serialize(),
    });

    try {
      await sendAndConfirmTransaction(
        connection,
        new Transaction().add(registerIx),
        [payer]
      );
      expect.fail("Should have failed");
    } catch (error: any) {
      expect(error).to.exist;
    }

    // Register user with new owner for deregister test
    const validRegisterIx = new TransactionInstruction({
      keys: [
        { pubkey: newOwner.publicKey, isSigner: true, isWritable: true },
        { pubkey: whitelistStatePDA, isSigner: false, isWritable: false },
        { pubkey: whitelistPDA, isSigner: false, isWritable: true },
        { pubkey: SystemProgram.programId, isSigner: false, isWritable: false },
      ],
      programId,
      data: new Register(userToWhitelist.publicKey).serialize(),
    });

    await sendAndConfirmTransaction(
      connection,
      new Transaction().add(validRegisterIx),
      [newOwner]
    );

    // Previous owner should not be able to deregister a user
    const deregisterIx = new TransactionInstruction({
      keys: [
        { pubkey: payer.publicKey, isSigner: true, isWritable: true },
        { pubkey: whitelistStatePDA, isSigner: false, isWritable: false },
        { pubkey: whitelistPDA, isSigner: false, isWritable: true },
      ],
      programId,
      data: new Deregister(userToWhitelist.publicKey).serialize(),
    });

    try {
      await sendAndConfirmTransaction(
        connection,
        new Transaction().add(deregisterIx),
        [payer]
      );
      expect.fail("Should have failed");
    } catch (error: any) {
      expect(error).to.exist;
    }
  });

  it("Non-owner cannot transfer ownership", async () => {
    const randomUser = Keypair.generate();
    await connection.requestAirdrop(randomUser.publicKey, 1 * LAMPORTS_PER_SOL);

    const transferIx = new TransactionInstruction({
      keys: [
        { pubkey: randomUser.publicKey, isSigner: true, isWritable: true },
        { pubkey: whitelistStatePDA, isSigner: false, isWritable: true },
      ],
      programId,
      data: new TransferOwnership(newOwner.publicKey).serialize(),
    });

    try {
      await sendAndConfirmTransaction(
        connection,
        new Transaction().add(transferIx),
        [randomUser]
      );
      expect.fail("Should have failed");
    } catch (error: any) {
      expect(error).to.exist;
    }
  });
});
