use anchor_lang::{prelude::Pubkey, pubkey};

/// Discriminator size in bytes
pub const DISCRIMINATOR: usize = 8;

/// mint value that indicates native transfers instead of spl transfers
pub const NATIVE_MINT: Pubkey = pubkey!("So11111111111111111111111111111111111111112");
