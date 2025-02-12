use anchor_lang::prelude::*;
use anchor_spl::associated_token::AssociatedToken;
use anchor_spl::token::{spl_token, Mint, Token, TokenAccount};

pub mod constants;
pub mod error;

use error::EscrowError;

declare_id!("AKEVm47qyu5E2LgBDrXifJjS2WJ7i4D1f9REzYvJEsLg");

#[program]
// #[warn(clippy::arithmetic_side_effects)]
pub mod fusion_swap {
    use super::*;

    pub fn initialize(
        ctx: Context<Initialize>,
        _order_id: u32,
        expiration_time: u32, // Order expiration time, unix timestamp
        src_amount: u64,      // Amount of tokens maker wants to sell
        dst_amount: u64,      // Amount of tokens maker wants in exchange
        traits: u8,
        receiver: Pubkey, // Owner of the account which will receive dst_token
        compact_fees: u64,
        protocol_dst_ata: Option<Pubkey>,
        integrator_dst_ata: Option<Pubkey>,
        estimated_dst_amount: u64,
    ) -> Result<()> {
        if src_amount == 0 || dst_amount == 0 {
            return err!(EscrowError::InvalidAmount);
        }

        if ctx.accounts.dst_mint.key() != spl_token::native_mint::id() && native_dst_asset(traits) {
            return err!(EscrowError::InconsistentNativeDstTrait);
        }

        let escrow = &mut ctx.accounts.escrow;

        let clock: Clock = Clock::get()?;
        if (expiration_time as i64) < clock.unix_timestamp {
            return err!(EscrowError::OrderExpired);
        }

        let protocol_fee = compact_fees as u16;
        let integrator_fee = (compact_fees >> 16) as u16;
        let surplus_percentage = (compact_fees >> 32) as u8;

        if surplus_percentage as u64 > constants::BASE_1E2 {
            return Err(EscrowError::InvalidProtocolSurplusFee.into());
        }

        // TODO: Uncomment this when dutch autions are implemented
        // if estimated_dst_amount <= dst_amount {
        //     return Err(EscrowError::InvalidEstimatedTakingAmount.into());
        // }

        if ((protocol_fee > 0 || surplus_percentage > 0) && protocol_dst_ata.is_none())
            || (protocol_fee == 0 && surplus_percentage == 0 && protocol_dst_ata.is_some())
        {
            return Err(EscrowError::InconsistentProtocolFeeConfig.into());
        }

        if (integrator_fee > 0 && integrator_dst_ata.is_none())
            || (integrator_fee == 0 && integrator_dst_ata.is_some())
        {
            return Err(EscrowError::InconsistentIntegratorFeeConfig.into());
        }

        escrow.set_inner(Escrow {
            src_amount,                            // Amount of tokens maker wants to sell
            src_remaining: src_amount,             // Remaining amount to be filled
            dst_amount,                            // Amount of tokens maker wants in exchange
            dst_mint: ctx.accounts.dst_mint.key(), // token maker wants in exchange
            authorized_user: ctx.accounts.authorized_user.as_ref().map(|acc| acc.key()),
            expiration_time,
            traits,
            receiver,
            protocol_fee,
            protocol_dst_ata,
            integrator_fee,
            integrator_dst_ata,
            surplus_percentage,
            estimated_dst_amount,
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
        )
    }

    pub fn fill(ctx: Context<Fill>, order_id: u32, amount: u64) -> Result<()> {
        // TODO: Check that signer has KYC token instead
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

        if amount == 0 {
            return err!(EscrowError::InvalidAmount);
        }

        // Check if partial fills are allowed if this is the case
        if ctx.accounts.escrow_src_ata.amount > amount
            && !allow_partial_fills(ctx.accounts.escrow.traits)
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

        let dst_amount = get_dst_amount(
            ctx.accounts.escrow.src_amount,
            ctx.accounts.escrow.dst_amount,
            amount,
        )?;

        let (protocol_fee_amount, integrator_fee_amount, actual_amount) = get_fee_amounts(
            ctx.accounts.escrow.integrator_fee as u64,
            ctx.accounts.escrow.protocol_fee as u64,
            ctx.accounts.escrow.surplus_percentage as u64,
            dst_amount,
            get_dst_amount(
                ctx.accounts.escrow.src_amount,
                ctx.accounts.escrow.estimated_dst_amount,
                amount,
            )?,
        )?;

        // Take protocol fee
        if protocol_fee_amount > 0 {
            let protocol_dst_ata = ctx
                .accounts
                .protocol_dst_ata
                .as_ref()
                .ok_or(EscrowError::InconsistentProtocolFeeConfig)?;

            anchor_spl::token::transfer(
                CpiContext::new(
                    ctx.accounts.system_program.to_account_info(),
                    anchor_spl::token::Transfer {
                        from: ctx.accounts.taker_dst_ata.to_account_info(),
                        to: protocol_dst_ata.to_account_info(),
                        authority: ctx.accounts.taker.to_account_info(),
                    },
                ),
                protocol_fee_amount,
            )?;
        }

        // Take integrator fee
        if integrator_fee_amount > 0 {
            let integrator_dst_ata = ctx
                .accounts
                .integrator_dst_ata
                .as_ref()
                .ok_or(EscrowError::InconsistentIntegratorFeeConfig)?;

            anchor_spl::token::transfer(
                CpiContext::new(
                    ctx.accounts.system_program.to_account_info(),
                    anchor_spl::token::Transfer {
                        from: ctx.accounts.taker_dst_ata.to_account_info(),
                        to: integrator_dst_ata.to_account_info(),
                        authority: ctx.accounts.taker.to_account_info(),
                    },
                ),
                integrator_fee_amount,
            )?;
        }

        // Taker => Maker
        if native_dst_asset(ctx.accounts.escrow.traits) {
            // Transfer SOL using System Program
            let ix = anchor_lang::solana_program::system_instruction::transfer(
                &ctx.accounts.taker.key(),
                &ctx.accounts.maker_receiver.key(),
                actual_amount,
            );
            anchor_lang::solana_program::program::invoke(
                &ix,
                &[
                    ctx.accounts.taker.to_account_info(),
                    ctx.accounts.maker_receiver.to_account_info(),
                    ctx.accounts.system_program.to_account_info(),
                ],
            )?;
        } else {
            // Transfer SPL tokens
            anchor_spl::token::transfer(
                CpiContext::new(
                    ctx.accounts.token_program.to_account_info(),
                    anchor_spl::token::Transfer {
                        from: ctx.accounts.taker_dst_ata.to_account_info(),
                        to: ctx.accounts.maker_dst_ata.to_account_info(),
                        authority: ctx.accounts.taker.to_account_info(),
                    },
                ),
                actual_amount,
            )?;
        }

        // Close escrow if all tokens are filled
        if ctx.accounts.escrow.src_remaining == 0 {
            close_escrow(
                ctx.accounts.token_program.to_account_info(),
                &ctx.accounts.escrow,
                ctx.accounts.escrow_src_ata.to_account_info(),
                ctx.accounts.maker.to_account_info(),
                order_id,
                ctx.bumps.escrow,
            )?;
        }

        Ok(())
    }

    pub fn cancel(ctx: Context<Cancel>, order_id: u32) -> Result<()> {
        // return remaining src tokens back to maker
        anchor_spl::token::transfer(
            CpiContext::new_with_signer(
                ctx.accounts.token_program.to_account_info(),
                anchor_spl::token::Transfer {
                    from: ctx.accounts.escrow_src_ata.to_account_info(),
                    to: ctx.accounts.maker_src_ata.to_account_info(),
                    authority: ctx.accounts.escrow.to_account_info(),
                },
                &[&[
                    "escrow".as_bytes(),
                    ctx.accounts.maker.key().as_ref(),
                    order_id.to_be_bytes().as_ref(),
                    &[ctx.bumps.escrow],
                ]],
            ),
            ctx.accounts.escrow_src_ata.amount,
        )?;

        close_escrow(
            ctx.accounts.token_program.to_account_info(),
            &ctx.accounts.escrow,
            ctx.accounts.escrow_src_ata.to_account_info(),
            ctx.accounts.maker.to_account_info(),
            order_id,
            ctx.bumps.escrow,
        )
    }
}

#[derive(Accounts)]
#[instruction(order_id: u32)]
pub struct Initialize<'info> {
    /// `maker`, who is willing to sell src token for dst token
    #[account(mut, signer)]
    maker: Signer<'info>,

    /// Source asset
    src_mint: Box<Account<'info, Mint>>,
    /// Destination asset
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
    #[account(mut, signer)]
    taker: Signer<'info>,

    /// CHECK: check is not necessary as maker is not spending any funds
    #[account(mut)]
    maker: AccountInfo<'info>,

    /// CHECK: maker_receiver only has to be equal to escrow parameter
    #[account(
        constraint = escrow.receiver == maker_receiver.key() @ EscrowError::SellerReceiverMismatch,
    )]
    maker_receiver: AccountInfo<'info>,

    /// Maker asset
    // TODO: Add src_mint to escrow or seeds
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

    /// Maker's ATA of dst_mint
    #[account(
        init_if_needed,
        payer = taker,
        associated_token::mint = dst_mint,
        associated_token::authority = maker_receiver
    )]
    maker_dst_ata: Box<Account<'info, TokenAccount>>,

    #[account(
        mut,
        constraint = Some(protocol_dst_ata.key()) == escrow.protocol_dst_ata @ EscrowError::InconsistentProtocolFeeConfig
    )]
    protocol_dst_ata: Option<Box<Account<'info, TokenAccount>>>,

    #[account(
        mut,
        constraint = Some(integrator_dst_ata.key()) == escrow.integrator_dst_ata @ EscrowError::InconsistentIntegratorFeeConfig
    )]
    integrator_dst_ata: Option<Box<Account<'info, TokenAccount>>>,

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
    #[account(mut, signer)]
    maker: Signer<'info>,

    /// Maker asset
    // TODO: Add src_mint to escrow or seeds
    src_mint: Account<'info, Mint>,

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
    protocol_fee: u16,
    protocol_dst_ata: Option<Pubkey>,
    integrator_fee: u16,
    integrator_dst_ata: Option<Pubkey>,
    surplus_percentage: u8,
    estimated_dst_amount: u64,
}

// Function to close the escrow account
fn close_escrow<'info>(
    token_program: AccountInfo<'info>,
    escrow: &Account<'info, Escrow>,
    escrow_src_ata: AccountInfo<'info>,
    maker: AccountInfo<'info>,
    order_id: u32,
    escrow_bump: u8,
) -> Result<()> {
    // Close escrow_src_ata account
    anchor_spl::token::close_account(CpiContext::new_with_signer(
        token_program,
        anchor_spl::token::CloseAccount {
            account: escrow_src_ata,
            destination: maker.to_account_info(),
            authority: escrow.to_account_info(),
        },
        &[&[
            "escrow".as_bytes(),
            maker.key().as_ref(),
            order_id.to_be_bytes().as_ref(),
            &[escrow_bump],
        ]],
    ))?;

    // Close escrow account
    escrow.close(maker)
}

// Function to get amount of `dst_mint` tokens that the taker should pay to the maker using the default formula
fn get_dst_amount(
    initial_src_amount: u64,
    initial_dst_amount: u64,
    src_amount: u64,
) -> Result<u64> {
    Ok(src_amount
        .checked_mul(initial_dst_amount)
        .ok_or(error::EscrowError::IntegerOverflow)?
        .div_ceil(initial_src_amount))
}

// Flag that defines if the order can be filled partially
pub fn allow_partial_fills(traits: u8) -> bool {
    traits & 0b00000001 != 0
}

// Flag that defines if the dst asset should be sent as native token
pub fn native_dst_asset(traits: u8) -> bool {
    traits & 0b00000010 != 0
}

fn get_fee_amounts(
    integrator_fee: u64,
    protocol_fee: u64,
    surplus_percentage: u64,
    dst_amount: u64,
    estimated_dst_amount: u64,
) -> Result<(u64, u64, u64)> {
    let integrator_fee_amount = dst_amount
        .checked_mul(integrator_fee)
        .ok_or(EscrowError::IntegerOverflow)?
        / constants::BASE_1E5;
    let mut protocol_fee_amount = dst_amount
        .checked_mul(protocol_fee)
        .ok_or(EscrowError::IntegerOverflow)?
        / constants::BASE_1E5;

    let actual_dst_amount = (dst_amount - protocol_fee_amount)
        .checked_sub(integrator_fee_amount)
        .ok_or(EscrowError::IntegerOverflow)?;

    if actual_dst_amount > estimated_dst_amount {
        protocol_fee_amount += (actual_dst_amount - estimated_dst_amount)
            .checked_mul(surplus_percentage)
            .ok_or(EscrowError::IntegerOverflow)?
            / constants::BASE_1E2;
    }

    Ok((
        protocol_fee_amount,
        integrator_fee_amount,
        dst_amount - protocol_fee_amount - integrator_fee_amount,
    ))
}
