use anchor_lang::prelude::*;
use common::constants;

pub mod error;
use error::WhitelistError;

declare_id!("6Lt7x1RwTWFdvPM1Hn58HdVfvdhW6rNS1fTSWgSZJcZy");

pub const WHITELIST_STATE_SEED: &[u8] = b"whitelist_state";
pub const WHITELIST_SEED: &[u8] = b"whitelist";

/// Program for managing whitelisted users for the Fusion Swap
#[program]
pub mod whitelist {
    use super::*;

    /// Initializes the whitelist with the owner
    pub fn initialize(ctx: Context<Initialize>) -> Result<()> {
        let whitelist_state = &mut ctx.accounts.whitelist_state;
        whitelist_state.owner = ctx.accounts.owner.key();
        Ok(())
    }

    /// Registers a new user to the whitelist
    pub fn register(_ctx: Context<Register>) -> Result<()> {
        Ok(())
    }

    /// Removes a user from the whitelist
    pub fn deregister(_ctx: Context<Deregister>) -> Result<()> {
        Ok(())
    }

    /// Transfers ownership of the whitelist to a new owner
    pub fn transfer_ownership(ctx: Context<TransferOwnership>) -> Result<()> {
        let whitelist_state = &mut ctx.accounts.whitelist_state;
        whitelist_state.owner = ctx.accounts.new_owner.key();
        Ok(())
    }
}

#[derive(Accounts)]
pub struct Initialize<'info> {
    #[account(mut)]
    pub owner: Signer<'info>,

    #[account(
        init,
        payer = owner,
        space = constants::DISCRIMINATOR + WhitelistState::INIT_SPACE,
        seeds = [WHITELIST_STATE_SEED],
        bump,
    )]
    pub whitelist_state: Account<'info, WhitelistState>,

    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct Register<'info> {
    #[account(mut)]
    pub owner: Signer<'info>,

    #[account(
      seeds = [WHITELIST_STATE_SEED],
      bump,
      // Ensures only the whitelist owner can register new users
      constraint = whitelist_state.owner == owner.key() @ WhitelistError::UnauthorizedOwner
    )]
    pub whitelist_state: Account<'info, WhitelistState>,

    #[account(
        init,
        payer = owner,
        space = constants::DISCRIMINATOR + Whitelisted::INIT_SPACE,
        seeds = [WHITELIST_SEED, user.key().as_ref()],
        bump,
    )]
    pub whitelisted: Account<'info, Whitelisted>,

    /// CHECK: This account is not read or written, just used for PDA creation
    pub user: AccountInfo<'info>,

    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct Deregister<'info> {
    #[account(mut)]
    pub owner: Signer<'info>,

    #[account(
      seeds = [WHITELIST_STATE_SEED],
      bump,
      // Ensures only the whitelist owner can register new users
      constraint = whitelist_state.owner == owner.key() @ WhitelistError::UnauthorizedOwner
    )]
    pub whitelist_state: Account<'info, WhitelistState>,

    #[account(
        mut,
        close = owner,
        seeds = [WHITELIST_SEED, user.key().as_ref()],
        bump,
    )]
    pub whitelisted: Account<'info, Whitelisted>,

    /// CHECK: This account is not read or written, just used for PDA creation
    pub user: AccountInfo<'info>,

    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct TransferOwnership<'info> {
    #[account(mut)]
    pub current_owner: Signer<'info>,
    /// CHECK: New owner's address is just stored
    pub new_owner: AccountInfo<'info>,
    #[account(
        mut,
        seeds = [WHITELIST_STATE_SEED],
        bump,
        // Ensures only the current owner can transfer ownership
        constraint = whitelist_state.owner == current_owner.key() @ WhitelistError::UnauthorizedOwner
    )]
    pub whitelist_state: Account<'info, WhitelistState>,
}

#[account]
#[derive(InitSpace)]
pub struct WhitelistState {
    pub owner: Pubkey,
}

#[account]
#[derive(InitSpace)]
pub struct Whitelisted {}
