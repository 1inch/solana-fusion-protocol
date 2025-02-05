use anchor_lang::prelude::*;
use anchor_spl::associated_token::AssociatedToken;
use anchor_spl::token::{Mint, Token, TokenAccount};

pub mod constants;
pub mod error;
pub mod utils;

declare_id!("AKEVm47qyu5E2LgBDrXifJjS2WJ7i4D1f9REzYvJEsLg");

#[program]
pub mod escrow {
    use error::EscrowError;

    use super::*;

    pub fn initialize(
        ctx: Context<Initialize>,
        _order_id: u32,
        expiration_time: u32, // Order expiration time, unix timestamp
        x_amount: u64,        // Amount of tokens maker wants to sell
        y_amount: u64,        // Amount of tokens maker wants in exchange
        escrow_traits: u8,
        sol_receiver: Pubkey, // Address to receive SOL when escrow is closed
        receiver: Pubkey,     // Owner of the account which will receive y_token
    ) -> Result<()> {
        let escrow = &mut ctx.accounts.escrow;

        let clock: Clock = Clock::get()?;
        if (expiration_time as i64) < clock.unix_timestamp {
            return err!(EscrowError::OrderExpired);
        }

        escrow.set_inner(Escrow {
            x_amount,                          // Amount of tokens maker wants to sell
            x_remaining: x_amount,             // Remaining amount to be filled
            y_amount,                          // Amount of tokens maker wants in exchange
            y_mint: ctx.accounts.y_mint.key(), // token maker wants in exchange
            authorized_user: ctx.accounts.authorized_user.as_ref().map(|acc| acc.key()),
            expiration_time,
            traits: escrow_traits,
            sol_receiver,
            receiver,
        });

        // Maker => Escrow
        anchor_spl::token::transfer(
            CpiContext::new(
                ctx.accounts.token_program.to_account_info(),
                anchor_spl::token::Transfer {
                    from: ctx.accounts.maker_x_token.to_account_info(),
                    to: ctx.accounts.escrowed_x_tokens.to_account_info(),
                    authority: ctx.accounts.maker.to_account_info(),
                },
            ),
            x_amount,
        )?;
        Ok(())
    }

    pub fn accept(ctx: Context<Accept>, order_id: u32, amount: u64) -> Result<()> {
        // if authorized_user is not set, allow exchange with any, otherwise check it
        if let Some(auth_user) = ctx.accounts.escrow.authorized_user {
            if auth_user != ctx.accounts.taker.key() {
                return err!(EscrowError::PrivateOrder);
            }
        }

        let clock: Clock = Clock::get()?;
        if (ctx.accounts.escrow.expiration_time as i64) < clock.unix_timestamp {
            return err!(EscrowError::OrderExpired);
        }

        if ctx.accounts.escrow.x_remaining < amount {
            return err!(EscrowError::NotEnoughTokensInEscrow);
        }

        // Check if partial fills are allowed if this is the case
        if !utils::allow_partial_fills(ctx.accounts.escrow.traits)
            && ctx.accounts.escrowed_x_tokens.amount > amount
        {
            return err!(EscrowError::PartialFillNotAllowed);
        }

        // Escrow => Taker
        anchor_spl::token::transfer(
            CpiContext::new_with_signer(
                ctx.accounts.token_program.to_account_info(),
                anchor_spl::token::Transfer {
                    from: ctx.accounts.escrowed_x_tokens.to_account_info(),
                    to: ctx.accounts.taker_x_tokens.to_account_info(),
                    authority: ctx.accounts.escrow.to_account_info(),
                },
                &[&[
                    "escrow6".as_bytes(),
                    ctx.accounts.maker.key().as_ref(),
                    order_id.to_be_bytes().as_ref(),
                    &[ctx.bumps.escrow],
                ]],
            ),
            amount,
        )?;

        // Update x_remaining
        ctx.accounts.escrow.x_remaining -= amount;

        // Check that owner of the account which will receive y_token is the same as the one set during escrow initialization
        if ctx.accounts.maker_receiver.key() != ctx.accounts.escrow.receiver {
            return err!(EscrowError::SellerReceiverMismatch);
        }

        let y_amount = utils::get_y_amount(
            ctx.accounts.escrow.x_amount,
            ctx.accounts.escrow.y_amount,
            amount,
        );

        // Taker => Maker
        anchor_spl::token::transfer(
            CpiContext::new(
                ctx.accounts.token_program.to_account_info(),
                anchor_spl::token::Transfer {
                    from: ctx.accounts.taker_y_tokens.to_account_info(),
                    to: ctx.accounts.maker_y_tokens.to_account_info(),
                    authority: ctx.accounts.taker.to_account_info(),
                },
            ),
            y_amount,
        )?;

        // Close escrow if multiple fills are not allowed or if all tokens are filled
        if !utils::allow_multiple_fills(ctx.accounts.escrow.traits)
            || ctx.accounts.escrow.x_remaining == 0
        {
            utils::close(
                ctx.accounts.token_program.to_account_info(),
                ctx.accounts.escrow.to_account_info(),
                ctx.accounts.escrowed_x_tokens.to_account_info(),
                ctx.accounts.escrowed_x_tokens.amount - amount,
                ctx.accounts.maker_x_token.to_account_info(),
                ctx.accounts.maker.to_account_info(),
                ctx.accounts.sol_receiver.to_account_info(),
                order_id,
                ctx.bumps.escrow,
            )?;
        }

        Ok(())
    }

    pub fn cancel(ctx: Context<Cancel>, order_id: u32) -> Result<()> {
        utils::close(
            ctx.accounts.token_program.to_account_info(),
            ctx.accounts.escrow.to_account_info(),
            ctx.accounts.escrowed_x_tokens.to_account_info(),
            ctx.accounts.escrowed_x_tokens.amount,
            ctx.accounts.maker_x_token.to_account_info(),
            ctx.accounts.maker.to_account_info(),
            ctx.accounts.sol_receiver.to_account_info(),
            order_id,
            ctx.bumps.escrow,
        )?;
        Ok(())
    }
}

#[derive(Accounts)]
#[instruction(order_id: u32)]
pub struct Initialize<'info> {
    /// `maker`, who is willing to sell token_x for token_y
    #[account(mut, signer)]
    maker: Signer<'info>,

    /// Maker asset
    x_mint: Box<Account<'info, Mint>>,
    /// Taker asset
    y_mint: Box<Account<'info, Mint>>,
    /// Account allowed to fill the order
    authorized_user: Option<AccountInfo<'info>>,

    /// Maker's ATA of x_mint
    #[account(
        mut,
        associated_token::mint = x_mint,
        associated_token::authority = maker
    )]
    maker_x_token: Box<Account<'info, TokenAccount>>,

    /// Account to store order conditions
    #[account(
        init,
        payer = maker,
        space = constants::DISCRIMINATOR + Escrow::INIT_SPACE,
        seeds = ["escrow6".as_bytes(), maker.key().as_ref(), order_id.to_be_bytes().as_ref()],
        bump,
    )]
    pub escrow: Box<Account<'info, Escrow>>,

    /// ATA of x_mint to store escrowed tokens
    #[account(
        init,
        payer = maker,
        associated_token::mint = x_mint,
        associated_token::authority = escrow,
    )]
    escrowed_x_tokens: Box<Account<'info, TokenAccount>>,

    associated_token_program: Program<'info, AssociatedToken>,
    token_program: Program<'info, Token>,
    rent: Sysvar<'info, Rent>,
    system_program: Program<'info, System>,
}

#[derive(Accounts)]
#[instruction(order_id: u32)]
pub struct Accept<'info> {
    /// `taker`, who buys `x_mint` for `y_mint`
    #[account(mut)]
    pub taker: Signer<'info>,

    /// CHECK: check is not necessary as maker is only used as an input to escrow address calculation
    pub maker: AccountInfo<'info>,

    /// CHECK: check is not necessary as maker_receiver is only used as a constraint to maker_y_tokens
    pub maker_receiver: AccountInfo<'info>,

    /// Maker asset
    pub x_mint: Box<Account<'info, Mint>>,
    /// Taker asset
    #[account(
        constraint = escrow.y_mint == y_mint.key(),
    )]
    pub y_mint: Box<Account<'info, Mint>>,

    /// Account to store order conditions
    #[account(
        mut,
        seeds = ["escrow6".as_bytes(), maker.key().as_ref(), order_id.to_be_bytes().as_ref()],
        bump,
    )]
    pub escrow: Box<Account<'info, Escrow>>,

    /// ATA of x_mint to store escrowed tokens
    #[account(
        mut,
        associated_token::mint = x_mint,
        associated_token::authority = escrow,
    )]
    pub escrowed_x_tokens: Box<Account<'info, TokenAccount>>,

    /// Maker's ATA of x_mint
    #[account(
        mut,
        associated_token::mint = x_mint,
        associated_token::authority = maker
    )]
    pub maker_x_token: Box<Account<'info, TokenAccount>>,

    /// CHECK: check is not necessary as sol_receiver is only used as an input to escrow address calculation
    /// This account will receive SOL when escrow is closed
    #[account(
        mut,
        constraint = escrow.sol_receiver == sol_receiver.key(),
    )]
    pub sol_receiver: AccountInfo<'info>,

    /// Maker's ATA of y_mint
    #[account(
        init_if_needed,
        payer = taker,
        associated_token::mint = y_mint,
        associated_token::authority = maker_receiver
    )]
    pub maker_y_tokens: Box<Account<'info, TokenAccount>>,

    // TODO initialize this account as well as 'maker_y_tokens'
    // this needs providing receiver address and adding
    // associated_token::mint = y_mint,
    // associated_token::authority = receiver
    // constraint
    /// Taker's ATA of x_mint
    #[account(
        mut,
        constraint = taker_x_tokens.mint.key() == x_mint.key()
    )]
    pub taker_x_tokens: Box<Account<'info, TokenAccount>>,

    /// Taker's ATA of y_mint
    #[account(
        mut,
        associated_token::mint = y_mint,
        associated_token::authority = taker
    )]
    pub taker_y_tokens: Box<Account<'info, TokenAccount>>,

    pub token_program: Program<'info, Token>,

    system_program: Program<'info, System>,
    associated_token_program: Program<'info, AssociatedToken>,
}

#[derive(Accounts)]
#[instruction(order_id: u32)]
pub struct Cancel<'info> {
    /// Account that created the escrow
    pub maker: Signer<'info>,

    /// Maker asset
    pub x_mint: Account<'info, Mint>,

    /// Account to store order conditions
    #[account(
        mut,
        close = maker,
        seeds = ["escrow6".as_bytes(), maker.key().as_ref(), order_id.to_be_bytes().as_ref()],
        bump,
    )]
    pub escrow: Box<Account<'info, Escrow>>,

    /// ATA of x_mint to store escrowed tokens
    #[account(
        mut,
        associated_token::mint = x_mint,
        associated_token::authority = escrow,
    )]
    pub escrowed_x_tokens: Account<'info, TokenAccount>,

    /// Maker's ATA of x_mint
    #[account(
        mut,
        associated_token::mint = x_mint,
        associated_token::authority = maker
    )]
    maker_x_token: Account<'info, TokenAccount>,

    /// CHECK: check is not necessary as sol_receiver is only used to receive SOL when escrow is closed
    #[account(
        mut,
        constraint = escrow.sol_receiver == sol_receiver.key(),
    )]
    pub sol_receiver: AccountInfo<'info>,

    token_program: Program<'info, Token>,
}

#[account]
#[derive(InitSpace)]
pub struct Escrow {
    y_mint: Pubkey,
    y_amount: u64,
    x_amount: u64,
    x_remaining: u64,
    expiration_time: u32,
    traits: u8,
    authorized_user: Option<Pubkey>,
    receiver: Pubkey,
    sol_receiver: Pubkey,
}
