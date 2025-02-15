use std::ops::{Deref, DerefMut};

use anchor_lang::prelude::*;
use anchor_spl::associated_token::AssociatedToken;
use anchor_spl::token::{spl_token, Mint, Token, TokenAccount};
use common::constants::{BASE_1E2, BASE_1E5, DISCRIMINATOR};
use dutch_auction::{calculate_rate_bump, DutchAuctionData};

pub mod dutch_auction;
pub mod error;

use error::EscrowError;

declare_id!("AKEVm47qyu5E2LgBDrXifJjS2WJ7i4D1f9REzYvJEsLg");

#[program]
pub mod fusion_swap {
    use super::*;

    pub fn create(ctx: Context<Create>, _order_id: u32, order: EscrowData) -> Result<()> {
        let order = order.init(&ctx);

        require!(
            order.src_amount != 0 && order.min_dst_amount != 0,
            EscrowError::InvalidAmount
        );

        require!(
            ctx.accounts.dst_mint.key() == spl_token::native_mint::id() || !order.native_dst_asset,
            EscrowError::InconsistentNativeDstTrait
        );

        let escrow = &mut ctx.accounts.escrow;

        require!(
            Clock::get()?.unix_timestamp <= order.expiration_time as i64,
            EscrowError::OrderExpired
        );

        require!(
            order.fee.surplus_percentage as u64 <= BASE_1E2,
            EscrowError::InvalidProtocolSurplusFee
        );

        require!(
            order.estimated_dst_amount >= order.min_dst_amount,
            EscrowError::InvalidEstimatedTakingAmount
        );

        if ((order.fee.protocol_fee > 0 || order.fee.surplus_percentage > 0)
            && order.fee.protocol_dst_ata.is_none())
            || (order.fee.protocol_fee == 0
                && order.fee.surplus_percentage == 0
                && order.fee.protocol_dst_ata.is_some())
        {
            return Err(EscrowError::InconsistentProtocolFeeConfig.into());
        }

        if (order.fee.integrator_fee > 0 && order.fee.integrator_dst_ata.is_none())
            || (order.fee.integrator_fee == 0 && order.fee.integrator_dst_ata.is_some())
        {
            return Err(EscrowError::InconsistentIntegratorFeeConfig.into());
        }

        escrow.set_inner(order.into());

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
            escrow.src_amount,
        )
    }

    pub fn fill(ctx: Context<Fill>, order_id: u32, amount: u64) -> Result<()> {
        require!(
            Clock::get()?.unix_timestamp <= ctx.accounts.escrow.expiration_time as i64,
            EscrowError::OrderExpired
        );

        require!(
            amount <= ctx.accounts.escrow.src_remaining,
            EscrowError::NotEnoughTokensInEscrow
        );

        require!(amount != 0, EscrowError::InvalidAmount);

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

        let min_dst_amount = get_dst_amount(
            ctx.accounts.escrow.src_amount,
            ctx.accounts.escrow.min_dst_amount,
            amount,
            Some(&ctx.accounts.escrow.dutch_auction_data),
        )?;

        let (protocol_fee_amount, integrator_fee_amount, actual_amount) = get_fee_amounts(
            ctx.accounts.escrow.fee.integrator_fee as u64,
            ctx.accounts.escrow.fee.protocol_fee as u64,
            ctx.accounts.escrow.fee.surplus_percentage as u64,
            min_dst_amount,
            get_dst_amount(
                ctx.accounts.escrow.src_amount,
                ctx.accounts.escrow.estimated_dst_amount,
                amount,
                None,
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
        if ctx.accounts.escrow.native_dst_asset {
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
            let maker_dst_ata = ctx
                .accounts
                .maker_dst_ata
                .as_ref()
                .ok_or(EscrowError::MissingMakerDstAta)?;

            // Transfer SPL tokens
            anchor_spl::token::transfer(
                CpiContext::new(
                    ctx.accounts.token_program.to_account_info(),
                    anchor_spl::token::Transfer {
                        from: ctx.accounts.taker_dst_ata.to_account_info(),
                        to: maker_dst_ata.to_account_info(),
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
pub struct Create<'info> {
    /// `maker`, who is willing to sell src token for dst token
    #[account(mut, signer)]
    maker: Signer<'info>,

    /// Source asset
    src_mint: Box<Account<'info, Mint>>,
    /// Destination asset
    dst_mint: Box<Account<'info, Mint>>,

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
        space = DISCRIMINATOR + Escrow::INIT_SPACE,
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
    system_program: Program<'info, System>,
}

#[derive(Accounts)]
#[instruction(order_id: u32)]
pub struct Fill<'info> {
    /// `taker`, who buys `src_mint` for `dst_mint`
    #[account(mut, signer)]
    taker: Signer<'info>,
    /// Account allowed to fill the order
    #[account(
        seeds = [whitelist::RESOLVER_ACCESS_SEED, taker.key().as_ref()],
        bump,
        seeds::program = whitelist::ID,
    )]
    resolver_access: Account<'info, whitelist::ResolverAccess>,

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
    maker_dst_ata: Option<Box<Account<'info, TokenAccount>>>,

    #[account(
        mut,
        constraint = Some(protocol_dst_ata.key()) == escrow.fee.protocol_dst_ata @ EscrowError::InconsistentProtocolFeeConfig
    )]
    protocol_dst_ata: Option<Box<Account<'info, TokenAccount>>>,

    #[account(
        mut,
        constraint = Some(integrator_dst_ata.key()) == escrow.fee.integrator_dst_ata @ EscrowError::InconsistentIntegratorFeeConfig
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

/// Configuration for fees applied to the escrow
#[derive(AnchorSerialize, AnchorDeserialize, Clone, InitSpace)]
pub struct FeeConfig {
    /// Protocol fee in basis points where `BASE_1E5` = 100%
    protocol_fee: u16,

    /// Integrator fee in basis points where `BASE_1E5` = 100%
    integrator_fee: u16,

    /// Percentage of positive slippage taken by the protocol as an additional fee.
    /// Value in basis points where `BASE_1E2` = 100%
    surplus_percentage: u8,

    /// Associated token account for collecting protocol fees
    protocol_dst_ata: Option<Pubkey>,

    /// Associated token account for collecting integrator fees
    integrator_dst_ata: Option<Pubkey>,
}

/// Core data structure for an escrow
#[derive(AnchorSerialize, AnchorDeserialize, Clone, InitSpace)]
pub struct EscrowData {
    /// The token that the maker wants to receive
    /// This field does not affect the created escrow in the `create` method, as it is always overwritten with the `dst_mint` account value.
    dst_mint: Pubkey,

    /// Minimum amount of `dst_mint` tokens the maker wants to receive
    min_dst_amount: u64,

    /// Amount of `src_mint` tokens the maker is offering to sell
    /// The `src_mint` token is not stored in Escrow; it is referenced from `Create` via `src_mint` account.
    src_amount: u64,

    /// Remaining amount of `src_mint` tokens available for fill
    /// This field does not affect the created escrow in the `create` method, as it is always overwritten with the `src_amount` value.
    src_remaining: u64,

    /// Unix timestamp indicating when the escrow expires   
    expiration_time: u32,

    /// Flag indicates whether `dst_mint` is native SOL (`true`) or an SPL token (`false`)
    native_dst_asset: bool,

    /// The wallet which will receive the `dst_mint` tokens
    receiver: Pubkey,

    /// See {FeeConfig}
    fee: FeeConfig,

    /// Estimated amount of `dst_mint` tokens the maker expects to receive.
    estimated_dst_amount: u64,

    /// Dutch auction parameters defining price adjustments over time
    dutch_auction_data: DutchAuctionData,
}

impl EscrowData {
    pub fn init(mut self, ctx: &Context<Create>) -> Self {
        self.dst_mint = ctx.accounts.dst_mint.key();
        self.src_remaining = self.src_amount;
        self
    }
}

#[account]
#[derive(InitSpace)]
pub struct Escrow {
    data: EscrowData,
}

// Implement Deref to allow Escrow to access EscrowData fields directly
impl Deref for Escrow {
    type Target = EscrowData;
    fn deref(&self) -> &Self::Target {
        &self.data
    }
}

impl DerefMut for Escrow {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.data
    }
}

impl From<EscrowData> for Escrow {
    fn from(data: EscrowData) -> Self {
        Escrow { data }
    }
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

// Function to get amount of `dst_mint` tokens that the taker should pay to the maker using default or the dutch auction formula
fn get_dst_amount(
    initial_src_amount: u64,
    initial_dst_amount: u64,
    src_amount: u64,
    opt_data: Option<&DutchAuctionData>,
) -> Result<u64> {
    let mut result = src_amount
        .checked_mul(initial_dst_amount)
        .ok_or(error::EscrowError::IntegerOverflow)?
        .div_ceil(initial_src_amount);

    if let Some(data) = opt_data {
        let rate_bump = calculate_rate_bump(Clock::get()?.unix_timestamp as u64, data);
        result = result
            .checked_mul(
                BASE_1E5
                    .checked_add(rate_bump)
                    .ok_or(error::EscrowError::IntegerOverflow)?,
            )
            .ok_or(error::EscrowError::IntegerOverflow)?
            .div_ceil(BASE_1E5);
    }
    Ok(result)
}

fn get_fee_amounts(
    integrator_fee: u64,
    protocol_fee: u64,
    surplus_percentage: u64,
    min_dst_amount: u64,
    estimated_dst_amount: u64,
) -> Result<(u64, u64, u64)> {
    let integrator_fee_amount = min_dst_amount
        .checked_mul(integrator_fee)
        .ok_or(EscrowError::IntegerOverflow)?
        / BASE_1E5;
    let mut protocol_fee_amount = min_dst_amount
        .checked_mul(protocol_fee)
        .ok_or(EscrowError::IntegerOverflow)?
        / BASE_1E5;

    let actual_dst_amount = (min_dst_amount - protocol_fee_amount)
        .checked_sub(integrator_fee_amount)
        .ok_or(EscrowError::IntegerOverflow)?;

    if actual_dst_amount > estimated_dst_amount {
        protocol_fee_amount += (actual_dst_amount - estimated_dst_amount)
            .checked_mul(surplus_percentage)
            .ok_or(EscrowError::IntegerOverflow)?
            / BASE_1E2;
    }

    Ok((
        protocol_fee_amount,
        integrator_fee_amount,
        min_dst_amount - protocol_fee_amount - integrator_fee_amount,
    ))
}
