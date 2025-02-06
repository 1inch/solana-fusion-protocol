use anchor_lang::prelude::*;
use anchor_spl::associated_token::AssociatedToken;
use anchor_spl::token::{Mint, Token, TokenAccount};

pub mod constants;
pub mod error;
pub mod utils;

declare_id!("AKEVm47qyu5E2LgBDrXifJjS2WJ7i4D1f9REzYvJEsLg");

#[program]
pub mod fusion_swap {
    use error::EscrowError;

    use super::*;

    pub fn initialize(
        ctx: Context<Initialize>,
        _order_id: u32,
        expiration_time: u32, // Order expiration time, unix timestamp
        src_amount: u64,      // Amount of tokens maker wants to sell
        dst_amount: u64,      // Amount of tokens maker wants in exchange
        escrow_traits: u8,
        receiver: Pubkey, // Owner of the account which will receive dst_token
    ) -> Result<()> {
        let escrow = &mut ctx.accounts.escrow;

        let clock: Clock = Clock::get()?;
        if (expiration_time as i64) < clock.unix_timestamp {
            return err!(EscrowError::OrderExpired);
        }

        escrow.set_inner(Escrow {
            src_amount,                            // Amount of tokens maker wants to sell
            src_remaining: src_amount,             // Remaining amount to be filled
            dst_amount,                            // Amount of tokens maker wants in exchange
            dst_mint: ctx.accounts.dst_mint.key(), // token maker wants in exchange
            authorized_user: ctx.accounts.authorized_user.as_ref().map(|acc| acc.key()),
            expiration_time,
            traits: escrow_traits,
            receiver,
        });

        // Maker => Escrow
        anchor_spl::token::transfer(
            CpiContext::new(
                ctx.accounts.token_program.to_account_info(),
                anchor_spl::token::Transfer {
                    from: ctx.accounts.maker_src_ata.to_account_info(),
                    to: ctx.accounts.escrow_src_ata.to_account_info(),
                    authority: ctx.accounts.maker.to_account_info(),
                },
            ),
            src_amount,
        )?;
        Ok(())
    }

    pub fn fill(ctx: Context<Fill>, order_id: u32, amount: u64) -> Result<()> {
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

        if ctx.accounts.escrow.src_remaining < amount {
            return err!(EscrowError::NotEnoughTokensInEscrow);
        }

        // Check if partial fills are allowed if this is the case
        if !utils::allow_partial_fills(ctx.accounts.escrow.traits)
            && ctx.accounts.escrow_src_ata.amount > amount
        {
            return err!(EscrowError::PartialFillNotAllowed);
        }

        // Escrow => Taker
        anchor_spl::token::transfer(
            CpiContext::new_with_signer(
                ctx.accounts.token_program.to_account_info(),
                anchor_spl::token::Transfer {
                    from: ctx.accounts.escrow_src_ata.to_account_info(),
                    to: ctx.accounts.taker_src_ata.to_account_info(),
                    authority: ctx.accounts.escrow.to_account_info(),
                },
                &[&[
                    "escrow".as_bytes(),
                    ctx.accounts.maker.key().as_ref(),
                    order_id.to_be_bytes().as_ref(),
                    &[ctx.bumps.escrow],
                ]],
            ),
            amount,
        )?;

        // Update src_remaining
        ctx.accounts.escrow.src_remaining -= amount;

        // Check that owner of the account which will receive dst_token is the same as the one set during escrow initialization
        if ctx.accounts.maker_receiver.key() != ctx.accounts.escrow.receiver {
            return err!(EscrowError::SellerReceiverMismatch);
        }

        let dst_amount = utils::get_dst_amount(
            ctx.accounts.escrow.src_amount,
            ctx.accounts.escrow.dst_amount,
            amount,
        );

        // Taker => Maker
        anchor_spl::token::transfer(
            CpiContext::new(
                ctx.accounts.token_program.to_account_info(),
                anchor_spl::token::Transfer {
                    from: ctx.accounts.taker_dst_ata.to_account_info(),
                    to: ctx.accounts.maker_dst_ata.to_account_info(),
                    authority: ctx.accounts.taker.to_account_info(),
                },
            ),
            dst_amount,
        )?;

        // Close escrow if multiple fills are not allowed or if all tokens are filled
        if !utils::allow_multiple_fills(ctx.accounts.escrow.traits)
            || ctx.accounts.escrow.src_remaining == 0
        {
            utils::close(
                ctx.accounts.token_program.to_account_info(),
                ctx.accounts.escrow.to_account_info(),
                ctx.accounts.escrow_src_ata.to_account_info(),
                ctx.accounts.escrow_src_ata.amount - amount,
                ctx.accounts.maker_src_ata.to_account_info(),
                ctx.accounts.maker.to_account_info(),
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
            ctx.accounts.escrow_src_ata.to_account_info(),
            ctx.accounts.escrow_src_ata.amount,
            ctx.accounts.maker_src_ata.to_account_info(),
            ctx.accounts.maker.to_account_info(),
            order_id,
            ctx.bumps.escrow,
        )?;
        Ok(())
    }
}

#[derive(Accounts)]
#[instruction(order_id: u32)]
pub struct Initialize<'info> {
    /// `maker`, who is willing to sell src token for dst token
    #[account(mut, signer)]
    maker: Signer<'info>,

    /// Maker asset
    src_mint: Box<Account<'info, Mint>>,
    /// Taker asset
    dst_mint: Box<Account<'info, Mint>>,
    /// Account allowed to fill the order
    authorized_user: Option<AccountInfo<'info>>,

    /// Maker's ATA of src_mint
    #[account(
        mut,
        associated_token::mint = src_mint,
        associated_token::authority = maker
    )]
    maker_src_ata: Box<Account<'info, TokenAccount>>,

    /// Account to store order conditions
    #[account(
        init,
        payer = maker,
        space = constants::DISCRIMINATOR + Escrow::INIT_SPACE,
        seeds = ["escrow".as_bytes(), maker.key().as_ref(), order_id.to_be_bytes().as_ref()],
        bump,
    )]
    escrow: Box<Account<'info, Escrow>>,

    /// ATA of src_mint to store escrowed tokens
    #[account(
        init,
        payer = maker,
        associated_token::mint = src_mint,
        associated_token::authority = escrow,
    )]
    escrow_src_ata: Box<Account<'info, TokenAccount>>,

    associated_token_program: Program<'info, AssociatedToken>,
    token_program: Program<'info, Token>,
    rent: Sysvar<'info, Rent>,
    system_program: Program<'info, System>,
}

#[derive(Accounts)]
#[instruction(order_id: u32)]
pub struct Fill<'info> {
    /// `taker`, who buys `src_mint` for `dst_mint`
    #[account(mut)]
    taker: Signer<'info>,

    /// CHECK: check is not necessary as maker is only used as an input to escrow address calculation
    #[account(mut)]
    maker: AccountInfo<'info>,

    /// CHECK: check is not necessary as maker_receiver is only used as a constraint to maker_dst_ata
    maker_receiver: AccountInfo<'info>,

    /// Maker asset
    src_mint: Box<Account<'info, Mint>>,
    /// Taker asset
    #[account(
        constraint = escrow.dst_mint == dst_mint.key(),
    )]
    dst_mint: Box<Account<'info, Mint>>,

    /// Account to store order conditions
    #[account(
        mut,
        seeds = ["escrow".as_bytes(), maker.key().as_ref(), order_id.to_be_bytes().as_ref()],
        bump,
    )]
    escrow: Box<Account<'info, Escrow>>,

    /// ATA of src_mint to store escrowed tokens
    #[account(
        mut,
        associated_token::mint = src_mint,
        associated_token::authority = escrow,
    )]
    escrow_src_ata: Box<Account<'info, TokenAccount>>,

    /// Maker's ATA of src_mint
    #[account(
        mut,
        associated_token::mint = src_mint,
        associated_token::authority = maker
    )]
    maker_src_ata: Box<Account<'info, TokenAccount>>,

    /// Maker's ATA of dst_mint
    #[account(
        init_if_needed,
        payer = taker,
        associated_token::mint = dst_mint,
        associated_token::authority = maker_receiver
    )]
    maker_dst_ata: Box<Account<'info, TokenAccount>>,

    // TODO initialize this account as well as 'maker_dst_ata'
    // this needs providing receiver address and adding
    // associated_token::mint = dst_mint,
    // associated_token::authority = receiver
    // constraint
    /// Taker's ATA of src_mint
    #[account(
        mut,
        constraint = taker_src_ata.mint.key() == src_mint.key()
    )]
    taker_src_ata: Box<Account<'info, TokenAccount>>,

    /// Taker's ATA of dst_mint
    #[account(
        mut,
        associated_token::mint = dst_mint,
        associated_token::authority = taker
    )]
    taker_dst_ata: Box<Account<'info, TokenAccount>>,

    token_program: Program<'info, Token>,

    system_program: Program<'info, System>,
    associated_token_program: Program<'info, AssociatedToken>,
}

#[derive(Accounts)]
#[instruction(order_id: u32)]
pub struct Cancel<'info> {
    /// Account that created the escrow
    #[account(mut)]
    maker: Signer<'info>,

    /// Maker asset
    src_mint: Account<'info, Mint>,

    /// Account to store order conditions
    #[account(
        mut,
        close = maker,
        seeds = ["escrow".as_bytes(), maker.key().as_ref(), order_id.to_be_bytes().as_ref()],
        bump,
    )]
    escrow: Box<Account<'info, Escrow>>,

    /// ATA of src_mint to store escrowed tokens
    #[account(
        mut,
        associated_token::mint = src_mint,
        associated_token::authority = escrow,
    )]
    escrow_src_ata: Account<'info, TokenAccount>,

    /// Maker's ATA of src_mint
    #[account(
        mut,
        associated_token::mint = src_mint,
        associated_token::authority = maker
    )]
    maker_src_ata: Account<'info, TokenAccount>,

    token_program: Program<'info, Token>,
}

#[account]
#[derive(InitSpace)]
pub struct Escrow {
    dst_mint: Pubkey,
    dst_amount: u64,
    src_amount: u64,
    src_remaining: u64,
    expiration_time: u32,
    traits: u8,
    authorized_user: Option<Pubkey>,
    receiver: Pubkey,
}
