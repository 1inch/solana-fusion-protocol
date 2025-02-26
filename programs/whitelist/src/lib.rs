use anchor_lang::prelude::*;
use common::constants::DISCRIMINATOR;

pub mod error;
use error::WhitelistError;

declare_id!("3cx4U4YnUNeDaQfqMkzw8AsVGtBXrcAbbjd1wPGMpMZc");

pub const WHITELIST_STATE_SEED: &[u8] = b"whitelist_state";
pub const RESOLVER_ACCESS_SEED: &[u8] = b"resolver_access";

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
    pub fn register(_ctx: Context<Register>, _user: Pubkey) -> Result<()> {
        Ok(())
    }

    /// Removes a user from the whitelist
    pub fn deregister(_ctx: Context<Deregister>, _user: Pubkey) -> Result<()> {
        Ok(())
    }

    /// Transfers ownership of the whitelist to a new owner
    pub fn transfer_ownership(ctx: Context<TransferOwnership>, _new_owner: Pubkey) -> Result<()> {
        let whitelist_state = &mut ctx.accounts.whitelist_state;
        whitelist_state.owner = _new_owner.key();
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
        space = DISCRIMINATOR + WhitelistState::INIT_SPACE,
        seeds = [WHITELIST_STATE_SEED],
        bump,
    )]
    pub whitelist_state: Account<'info, WhitelistState>,

    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
#[instruction(user: Pubkey)]
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
        space = DISCRIMINATOR + ResolverAccess::INIT_SPACE,
        seeds = [RESOLVER_ACCESS_SEED, user.key().as_ref()],
        bump,
    )]
    pub resolver_access: Account<'info, ResolverAccess>,

    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
#[instruction(user: Pubkey)]
pub struct Deregister<'info> {
    #[account(mut)]
    pub owner: Signer<'info>,

    #[account(
      seeds = [WHITELIST_STATE_SEED],
      bump,
      // Ensures only the whitelist owner can deregister users from the whitelist
      constraint = whitelist_state.owner == owner.key() @ WhitelistError::UnauthorizedOwner
    )]
    pub whitelist_state: Account<'info, WhitelistState>,

    #[account(
        mut,
        close = owner,
        seeds = [RESOLVER_ACCESS_SEED, user.key().as_ref()],
        bump,
    )]
    pub resolver_access: Account<'info, ResolverAccess>,

    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct TransferOwnership<'info> {
    #[account(mut)]
    pub current_owner: Signer<'info>,
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
pub struct ResolverAccess {}
