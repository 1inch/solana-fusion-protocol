use anchor_lang::prelude::*;
use anchor_lang::solana_program::hash::hash;
use anchor_spl::{
    associated_token::AssociatedToken,
    token::spl_token::native_mint,
    token_interface::{
        close_account, transfer_checked, CloseAccount, Mint, TokenAccount, TokenInterface,
        TransferChecked,
    },
};
use auction::{calculate_premium_multiplier, calculate_rate_bump, AuctionData};
use muldiv::MulDiv;

pub mod auction;
pub mod error;

use error::FusionError;

declare_id!("9CnwB8RDNtRzRcxvkNqwgatRDENBCh2f56HgJLPStn8S");

pub const BASE_1E2: u64 = 100;
pub const BASE_1E3: u64 = 1000;
pub const BASE_1E5: u64 = 100_000;

#[program]
pub mod fusion_swap {
    use super::*;

    pub fn create(ctx: Context<Create>, order: ReducedOrderConfig) -> Result<()> {
        require!(
            order.src_amount != 0 && order.min_dst_amount != 0,
            FusionError::InvalidAmount
        );

        // we support only original spl_token::native_mint
        require!(
            ctx.accounts.dst_mint.key() == native_mint::id() || !order.native_dst_asset,
            FusionError::InconsistentNativeDstTrait
        );

        require!(
            Clock::get()?.unix_timestamp <= order.expiration_time as i64,
            FusionError::OrderExpired
        );

        require!(
            order.fee.surplus_percentage as u64 <= BASE_1E2,
            FusionError::InvalidProtocolSurplusFee
        );

        require!(
            order.estimated_dst_amount >= order.min_dst_amount,
            FusionError::InvalidEstimatedTakingAmount
        );

        // Iff protocol fee or surplus is positive, protocol_dst_ata must be set
        require!(
            (order.fee.protocol_fee > 0 || order.fee.surplus_percentage > 0)
                == ctx.accounts.protocol_dst_ata.is_some(),
            FusionError::InconsistentProtocolFeeConfig
        );

        // Iff integrator fee is positive, integrator_dst_ata must be set
        require!(
            (order.fee.integrator_fee > 0) == ctx.accounts.integrator_dst_ata.is_some(),
            FusionError::InconsistentIntegratorFeeConfig
        );

        // Maker => Escrow
        transfer_checked(
            CpiContext::new(
                ctx.accounts.src_token_program.to_account_info(),
                TransferChecked {
                    from: ctx.accounts.maker_src_ata.to_account_info(),
                    mint: ctx.accounts.src_mint.to_account_info(),
                    to: ctx.accounts.escrow_src_ata.to_account_info(),
                    authority: ctx.accounts.maker.to_account_info(),
                },
            ),
            order.src_amount,
            ctx.accounts.src_mint.decimals,
        )
    }

    pub fn fill(ctx: Context<Fill>, reduced_order: ReducedOrderConfig, amount: u64) -> Result<()> {
        require!(
            Clock::get()?.unix_timestamp <= reduced_order.expiration_time as i64,
            FusionError::OrderExpired
        );

        require!(
            amount <= ctx.accounts.escrow_src_ata.amount,
            FusionError::NotEnoughTokensInEscrow
        );

        require!(amount != 0, FusionError::InvalidAmount);

        let order = build_order_from_reduced(
            &reduced_order,
            ctx.accounts.src_mint.key(),
            ctx.accounts.dst_mint.key(),
            ctx.accounts.maker_receiver.key(),
            ctx.accounts.protocol_dst_ata.as_ref().map(|ata| ata.key()),
            ctx.accounts
                .integrator_dst_ata
                .as_ref()
                .map(|ata| ata.key()),
        );

        // Escrow => Taker
        transfer_checked(
            CpiContext::new_with_signer(
                ctx.accounts.src_token_program.to_account_info(),
                TransferChecked {
                    from: ctx.accounts.escrow_src_ata.to_account_info(),
                    mint: ctx.accounts.src_mint.to_account_info(),
                    to: ctx.accounts.taker_src_ata.to_account_info(),
                    authority: ctx.accounts.escrow.to_account_info(),
                },
                &[&[
                    "escrow".as_bytes(),
                    ctx.accounts.maker.key().as_ref(),
                    &order_hash(&order)?,
                    &[ctx.bumps.escrow],
                ]],
            ),
            amount,
            ctx.accounts.src_mint.decimals,
        )?;

        let dst_amount = get_dst_amount(
            order.src_amount,
            order.min_dst_amount,
            amount,
            Some(&order.dutch_auction_data),
        )?;

        let (protocol_fee_amount, integrator_fee_amount, maker_dst_amount) = get_fee_amounts(
            order.fee.integrator_fee,
            order.fee.protocol_fee,
            order.fee.surplus_percentage,
            dst_amount,
            get_dst_amount(order.src_amount, order.estimated_dst_amount, amount, None)?,
        )?;

        // Take protocol fee
        if protocol_fee_amount > 0 {
            let protocol_dst_ata = ctx
                .accounts
                .protocol_dst_ata
                .as_ref()
                .ok_or(FusionError::InconsistentProtocolFeeConfig)?;

            transfer_checked(
                CpiContext::new(
                    ctx.accounts.dst_token_program.to_account_info(),
                    TransferChecked {
                        from: ctx.accounts.taker_dst_ata.to_account_info(),
                        mint: ctx.accounts.dst_mint.to_account_info(),
                        to: protocol_dst_ata.to_account_info(),
                        authority: ctx.accounts.taker.to_account_info(),
                    },
                ),
                protocol_fee_amount,
                ctx.accounts.dst_mint.decimals,
            )?;
        }

        // Take integrator fee
        if integrator_fee_amount > 0 {
            let integrator_dst_ata = ctx
                .accounts
                .integrator_dst_ata
                .as_ref()
                .ok_or(FusionError::InconsistentIntegratorFeeConfig)?;

            transfer_checked(
                CpiContext::new(
                    ctx.accounts.dst_token_program.to_account_info(),
                    TransferChecked {
                        from: ctx.accounts.taker_dst_ata.to_account_info(),
                        mint: ctx.accounts.dst_mint.to_account_info(),
                        to: integrator_dst_ata.to_account_info(),
                        authority: ctx.accounts.taker.to_account_info(),
                    },
                ),
                integrator_fee_amount,
                ctx.accounts.dst_mint.decimals,
            )?;
        }

        // Taker => Maker
        if order.native_dst_asset {
            // Transfer native SOL
            anchor_lang::system_program::transfer(
                CpiContext::new(
                    ctx.accounts.system_program.to_account_info(),
                    anchor_lang::system_program::Transfer {
                        from: ctx.accounts.taker.to_account_info(),
                        to: ctx.accounts.maker_receiver.to_account_info(),
                    },
                ),
                maker_dst_amount,
            )?;
        } else {
            let maker_dst_ata = ctx
                .accounts
                .maker_dst_ata
                .as_ref()
                .ok_or(FusionError::MissingMakerDstAta)?;

            // Transfer SPL tokens
            transfer_checked(
                CpiContext::new(
                    ctx.accounts.dst_token_program.to_account_info(),
                    TransferChecked {
                        from: ctx.accounts.taker_dst_ata.to_account_info(),
                        mint: ctx.accounts.dst_mint.to_account_info(),
                        to: maker_dst_ata.to_account_info(),
                        authority: ctx.accounts.taker.to_account_info(),
                    },
                ),
                maker_dst_amount,
                ctx.accounts.dst_mint.decimals,
            )?;
        }

        // Close escrow if all tokens are filled
        if ctx.accounts.escrow_src_ata.amount == amount {
            close_account(CpiContext::new_with_signer(
                ctx.accounts.src_token_program.to_account_info(),
                CloseAccount {
                    account: ctx.accounts.escrow_src_ata.to_account_info(),
                    destination: ctx.accounts.maker.to_account_info(),
                    authority: ctx.accounts.escrow.to_account_info(),
                },
                &[&[
                    "escrow".as_bytes(),
                    ctx.accounts.maker.key().as_ref(),
                    &order_hash(&order)?,
                    &[ctx.bumps.escrow],
                ]],
            ))?;
        }

        Ok(())
    }

    pub fn cancel(ctx: Context<Cancel>, order_hash: [u8; 32]) -> Result<()> {
        // return remaining src tokens back to maker
        transfer_checked(
            CpiContext::new_with_signer(
                ctx.accounts.src_token_program.to_account_info(),
                TransferChecked {
                    from: ctx.accounts.escrow_src_ata.to_account_info(),
                    mint: ctx.accounts.src_mint.to_account_info(),
                    to: ctx.accounts.maker_src_ata.to_account_info(),
                    authority: ctx.accounts.escrow.to_account_info(),
                },
                &[&[
                    "escrow".as_bytes(),
                    ctx.accounts.maker.key().as_ref(),
                    &order_hash,
                    &[ctx.bumps.escrow],
                ]],
            ),
            ctx.accounts.escrow_src_ata.amount,
            ctx.accounts.src_mint.decimals,
        )?;

        close_account(CpiContext::new_with_signer(
            ctx.accounts.src_token_program.to_account_info(),
            CloseAccount {
                account: ctx.accounts.escrow_src_ata.to_account_info(),
                destination: ctx.accounts.maker.to_account_info(),
                authority: ctx.accounts.escrow.to_account_info(),
            },
            &[&[
                "escrow".as_bytes(),
                ctx.accounts.maker.key().as_ref(),
                &order_hash,
                &[ctx.bumps.escrow],
            ]],
        ))
    }

    pub fn cancel_by_resolver(
        ctx: Context<CancelByResolver>,
        reduced_order: ReducedOrderConfig,
    ) -> Result<()> {
        require!(
            reduced_order.fee.min_cancellation_premium > 0,
            FusionError::CancelOrderByResolverIsForbidden
        );
        let current_timestamp = Clock::get()?.unix_timestamp;
        require!(
            current_timestamp >= reduced_order.expiration_time as i64,
            FusionError::OrderNotExpired
        );

        // Calculate the total cancellation premium (base + auction premium)
        let premium_bump = calculate_premium_multiplier(
            current_timestamp as u32,
            reduced_order.expiration_time,
            reduced_order.cancellation_auction_duration,
            reduced_order.fee.max_cancellation_multiplier,
        );

        let total_cancellation_premium = reduced_order
            .fee
            .min_cancellation_premium
            .mul_div_ceil(BASE_1E3 + premium_bump, BASE_1E3)
            .ok_or(ProgramError::ArithmeticOverflow)?;
        require!(
            total_cancellation_premium <= ctx.accounts.escrow_src_ata.amount,
            FusionError::InvalidCancellationFee
        );

        let order = build_order_from_reduced(
            &reduced_order,
            ctx.accounts.src_mint.key(),
            ctx.accounts.dst_mint.key(),
            ctx.accounts.maker_receiver.key(),
            ctx.accounts.protocol_dst_ata.as_ref().map(|ata| ata.key()),
            ctx.accounts
                .integrator_dst_ata
                .as_ref()
                .map(|ata| ata.key()),
        );

        let order_hash = order_hash(&order)?;

        // Transfer cancellation fee to resolver
        transfer_checked(
            CpiContext::new_with_signer(
                ctx.accounts.src_token_program.to_account_info(),
                TransferChecked {
                    from: ctx.accounts.escrow_src_ata.to_account_info(),
                    mint: ctx.accounts.src_mint.to_account_info(),
                    to: ctx.accounts.resolver_src_ata.to_account_info(),
                    authority: ctx.accounts.escrow.to_account_info(),
                },
                &[&[
                    "escrow".as_bytes(),
                    ctx.accounts.maker.key().as_ref(),
                    &order_hash,
                    &[ctx.bumps.escrow],
                ]],
            ),
            total_cancellation_premium,
            ctx.accounts.src_mint.decimals,
        )?;

        let remaining_src_amount = ctx.accounts.escrow_src_ata.amount - total_cancellation_premium;

        // Return the remaining src tokens back to the maker if any
        if remaining_src_amount > 0 {
            transfer_checked(
                CpiContext::new_with_signer(
                    ctx.accounts.src_token_program.to_account_info(),
                    TransferChecked {
                        from: ctx.accounts.escrow_src_ata.to_account_info(),
                        mint: ctx.accounts.src_mint.to_account_info(),
                        to: ctx.accounts.maker_src_ata.to_account_info(),
                        authority: ctx.accounts.escrow.to_account_info(),
                    },
                    &[&[
                        "escrow".as_bytes(),
                        ctx.accounts.maker.key().as_ref(),
                        &order_hash,
                        &[ctx.bumps.escrow],
                    ]],
                ),
                remaining_src_amount,
                ctx.accounts.src_mint.decimals,
            )?;
        }

        close_account(CpiContext::new_with_signer(
            ctx.accounts.src_token_program.to_account_info(),
            CloseAccount {
                account: ctx.accounts.escrow_src_ata.to_account_info(),
                destination: ctx.accounts.maker.to_account_info(),
                authority: ctx.accounts.escrow.to_account_info(),
            },
            &[&[
                "escrow".as_bytes(),
                ctx.accounts.maker.key().as_ref(),
                &order_hash,
                &[ctx.bumps.escrow],
            ]],
        ))
    }
}

#[derive(Accounts)]
#[instruction(order: ReducedOrderConfig)]
pub struct Create<'info> {
    /// `maker`, who is willing to sell src token for dst token
    #[account(mut, signer)]
    maker: Signer<'info>,

    /// Source asset
    src_mint: Box<InterfaceAccount<'info, Mint>>,
    /// Destination asset
    dst_mint: Box<InterfaceAccount<'info, Mint>>,

    /// Maker's ATA of src_mint
    #[account(
        mut,
        associated_token::mint = src_mint,
        associated_token::authority = maker,
        associated_token::token_program = src_token_program,
    )]
    maker_src_ata: Box<InterfaceAccount<'info, TokenAccount>>,

    /// CHECK: maker_receiver only has to be equal to escrow parameter
    maker_receiver: AccountInfo<'info>,

    /// Account to store order conditions
    #[account(
        seeds = [
            "escrow".as_bytes(),
            maker.key().as_ref(),
            &order_hash(&build_order_from_reduced(
                &order,
                src_mint.key(),
                dst_mint.key(),
                maker_receiver.key(),
                protocol_dst_ata.clone().map(|ata| ata.key()),
                integrator_dst_ata.clone().map(|ata| ata.key()),
            ))?,
        ],
        bump,
    )]
    /// CHECK: check is not needed here as we never initialize the account
    escrow: AccountInfo<'info>,

    /// ATA of src_mint to store escrowed tokens
    #[account(
        init,
        payer = maker,
        associated_token::mint = src_mint,
        associated_token::authority = escrow,
        associated_token::token_program = src_token_program,
    )]
    escrow_src_ata: Box<InterfaceAccount<'info, TokenAccount>>,

    #[account(
        constraint = protocol_dst_ata.mint == dst_mint.key() @ FusionError::InconsistentProtocolFeeConfig
    )]
    protocol_dst_ata: Option<Box<InterfaceAccount<'info, TokenAccount>>>,

    #[account(
        constraint = integrator_dst_ata.mint == dst_mint.key() @ FusionError::InconsistentIntegratorFeeConfig
    )]
    integrator_dst_ata: Option<Box<InterfaceAccount<'info, TokenAccount>>>,

    associated_token_program: Program<'info, AssociatedToken>,
    src_token_program: Interface<'info, TokenInterface>,
    system_program: Program<'info, System>,
}

#[derive(Accounts)]
#[instruction(order: ReducedOrderConfig)]
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
    maker_receiver: AccountInfo<'info>,

    /// Maker asset
    src_mint: Box<InterfaceAccount<'info, Mint>>,
    /// Taker asset
    dst_mint: Box<InterfaceAccount<'info, Mint>>,

    /// Account to store order conditions
    #[account(
        seeds = [
            "escrow".as_bytes(),
            maker.key().as_ref(),
            &order_hash(&build_order_from_reduced(
                &order,
                src_mint.key(),
                dst_mint.key(),
                maker_receiver.key(),
                protocol_dst_ata.clone().map(|ata| ata.key()),
                integrator_dst_ata.clone().map(|ata| ata.key()),
            ))?,
        ],
        bump,
    )]
    /// CHECK: check is not needed here as we never initialize the account
    escrow: AccountInfo<'info>,

    /// ATA of src_mint to store escrowed tokens
    #[account(
        mut,
        associated_token::mint = src_mint,
        associated_token::authority = escrow,
        associated_token::token_program = src_token_program,
    )]
    escrow_src_ata: Box<InterfaceAccount<'info, TokenAccount>>,

    /// Maker's ATA of dst_mint
    #[account(
        init_if_needed,
        payer = taker,
        associated_token::mint = dst_mint,
        associated_token::authority = maker_receiver,
        associated_token::token_program = dst_token_program,
    )]
    maker_dst_ata: Option<Box<InterfaceAccount<'info, TokenAccount>>>,

    #[account(mut)]
    protocol_dst_ata: Option<Box<InterfaceAccount<'info, TokenAccount>>>,

    #[account(mut)]
    integrator_dst_ata: Option<Box<InterfaceAccount<'info, TokenAccount>>>,

    /// Taker's ATA of src_mint
    #[account(
        mut,
        constraint = taker_src_ata.mint.key() == src_mint.key()
    )]
    taker_src_ata: Box<InterfaceAccount<'info, TokenAccount>>,

    /// Taker's ATA of dst_mint
    #[account(
        mut,
        associated_token::mint = dst_mint,
        associated_token::authority = taker,
        associated_token::token_program = dst_token_program,
    )]
    taker_dst_ata: Box<InterfaceAccount<'info, TokenAccount>>,

    src_token_program: Interface<'info, TokenInterface>,
    dst_token_program: Interface<'info, TokenInterface>,
    system_program: Program<'info, System>,
    associated_token_program: Program<'info, AssociatedToken>,
}

#[derive(Accounts)]
#[instruction(order_hash: [u8; 32])]
pub struct Cancel<'info> {
    /// Account that created the escrow
    #[account(mut, signer)]
    maker: Signer<'info>,

    /// Maker asset
    src_mint: InterfaceAccount<'info, Mint>,

    /// Account to store order conditions
    #[account(
        seeds = [
            "escrow".as_bytes(),
            maker.key().as_ref(),
            &order_hash,
        ],
        bump,
    )]
    /// CHECK: check is not needed here as we never initialize the account
    escrow: AccountInfo<'info>,

    /// ATA of src_mint to store escrowed tokens
    #[account(
        mut,
        associated_token::mint = src_mint,
        associated_token::authority = escrow,
        associated_token::token_program = src_token_program,
    )]
    escrow_src_ata: InterfaceAccount<'info, TokenAccount>,

    /// Maker's ATA of src_mint
    #[account(
        mut,
        associated_token::mint = src_mint,
        associated_token::authority = maker,
        associated_token::token_program = src_token_program,
    )]
    maker_src_ata: InterfaceAccount<'info, TokenAccount>,

    src_token_program: Interface<'info, TokenInterface>,
}

#[derive(Accounts)]
#[instruction(order: ReducedOrderConfig)]
pub struct CancelByResolver<'info> {
    /// Account that cancels the escrow
    #[account(mut, signer)]
    resolver: Signer<'info>,

    /// Account allowed to cancel the order
    #[account(
        seeds = [whitelist::RESOLVER_ACCESS_SEED, resolver.key().as_ref()],
        bump,
        seeds::program = whitelist::ID,
    )]
    resolver_access: Account<'info, whitelist::ResolverAccess>,

    /// CHECK: check is not necessary as maker is not spending any funds
    #[account(mut)]
    maker: AccountInfo<'info>,

    /// CHECK: maker_receiver only has to be equal to escrow parameter
    maker_receiver: AccountInfo<'info>,

    /// Maker asset
    src_mint: InterfaceAccount<'info, Mint>,

    /// Taker asset
    dst_mint: Box<InterfaceAccount<'info, Mint>>,

    /// Account to store order conditions
    #[account(
        seeds = [
            "escrow".as_bytes(),
            maker.key().as_ref(),
            &order_hash(&build_order_from_reduced(
                &order,
                src_mint.key(),
                dst_mint.key(),
                maker_receiver.key(),
                protocol_dst_ata.clone().map(|ata| ata.key()),
                integrator_dst_ata.clone().map(|ata| ata.key()),
            ))?,
        ],
        bump,
    )]
    /// CHECK: check is not needed here as we never initialize the account
    escrow: AccountInfo<'info>,

    /// ATA of src_mint to store escrowed tokens
    #[account(
        mut,
        associated_token::mint = src_mint,
        associated_token::authority = escrow,
        associated_token::token_program = src_token_program,
    )]
    escrow_src_ata: InterfaceAccount<'info, TokenAccount>,

    /// Maker's ATA of src_mint
    #[account(
        mut,
        associated_token::mint = src_mint,
        associated_token::authority = maker,
        associated_token::token_program = src_token_program,
    )]
    maker_src_ata: InterfaceAccount<'info, TokenAccount>,

    protocol_dst_ata: Option<Box<InterfaceAccount<'info, TokenAccount>>>,

    integrator_dst_ata: Option<Box<InterfaceAccount<'info, TokenAccount>>>,

    /// Resolver's ATA of src_mint
    #[account(
        mut,
        associated_token::mint = src_mint,
        associated_token::authority = resolver,
        associated_token::token_program = src_token_program,
    )]
    resolver_src_ata: InterfaceAccount<'info, TokenAccount>,

    src_token_program: Interface<'info, TokenInterface>,
}

/// Configuration for fees applied to the escrow
#[derive(AnchorSerialize, AnchorDeserialize, Clone, InitSpace)]
pub struct FeeConfig {
    protocol_dst_ata: Option<Pubkey>,
    integrator_dst_ata: Option<Pubkey>,

    /// Protocol fee in basis points where `BASE_1E5` = 100%
    protocol_fee: u16,

    /// Integrator fee in basis points where `BASE_1E5` = 100%
    integrator_fee: u16,

    /// Percentage of positive slippage taken by the protocol as an additional fee.
    /// Value in basis points where `BASE_1E2` = 100%
    surplus_percentage: u8,

    /// Fee charged to the maker if the order is cancelled by resolver
    /// Value in absolute token amount
    min_cancellation_premium: u64,

    /// Maximum cancellation premium multiplier
    /// Value in basis points where `BASE_1E3` = 100%
    max_cancellation_multiplier: u16,
}

/// Configuration for fees applied to the escrow
#[derive(AnchorSerialize, AnchorDeserialize, Clone, InitSpace)]
pub struct ReducedFeeConfig {
    /// Protocol fee in basis points where `BASE_1E5` = 100%
    protocol_fee: u16,

    /// Integrator fee in basis points where `BASE_1E5` = 100%
    integrator_fee: u16,

    /// Percentage of positive slippage taken by the protocol as an additional fee.
    /// Value in basis points where `BASE_1E2` = 100%
    surplus_percentage: u8,

    /// Fee charged to the maker if the order is cancelled by resolver
    /// Value in absolute token amount
    min_cancellation_premium: u64,
    /// Maximum cancellation premium multiplier
    /// Value in basis points where `BASE_1E3` = 100%
    max_cancellation_multiplier: u16,
}

#[derive(AnchorSerialize, AnchorDeserialize)]
pub struct OrderConfig {
    id: u32,
    src_amount: u64,
    min_dst_amount: u64,
    estimated_dst_amount: u64,
    expiration_time: u32,
    native_dst_asset: bool,
    receiver: Pubkey,
    fee: FeeConfig,
    dutch_auction_data: AuctionData,
    cancellation_auction_duration: u32,
    src_mint: Pubkey,
    dst_mint: Pubkey,
}

#[derive(AnchorSerialize, AnchorDeserialize)]
pub struct ReducedOrderConfig {
    id: u32,
    src_amount: u64,
    min_dst_amount: u64,
    estimated_dst_amount: u64,
    expiration_time: u32,
    native_dst_asset: bool,
    fee: ReducedFeeConfig,
    dutch_auction_data: AuctionData,
    cancellation_auction_duration: u32,
}

fn order_hash(order: &OrderConfig) -> Result<[u8; 32]> {
    Ok(hash(order.try_to_vec()?.as_ref()).to_bytes())
}

// Function to get amount of `dst_mint` tokens that the taker should pay to the maker using default or the dutch auction formula
fn get_dst_amount(
    initial_src_amount: u64,
    initial_dst_amount: u64,
    src_amount: u64,
    opt_data: Option<&AuctionData>,
) -> Result<u64> {
    let mut result = initial_dst_amount
        .mul_div_ceil(src_amount, initial_src_amount)
        .ok_or(ProgramError::ArithmeticOverflow)?;

    if let Some(data) = opt_data {
        let rate_bump = calculate_rate_bump(Clock::get()?.unix_timestamp as u64, data);
        result = result
            .mul_div_ceil(BASE_1E5 + rate_bump, BASE_1E5)
            .ok_or(ProgramError::ArithmeticOverflow)?;
    }
    Ok(result)
}

fn get_fee_amounts(
    integrator_fee: u16,
    protocol_fee: u16,
    surplus_percentage: u8,
    dst_amount: u64,
    estimated_dst_amount: u64,
) -> Result<(u64, u64, u64)> {
    let integrator_fee_amount = dst_amount
        .mul_div_floor(integrator_fee as u64, BASE_1E5)
        .ok_or(ProgramError::ArithmeticOverflow)?;

    let mut protocol_fee_amount = dst_amount
        .mul_div_floor(protocol_fee as u64, BASE_1E5)
        .ok_or(ProgramError::ArithmeticOverflow)?;

    let actual_dst_amount = (dst_amount - protocol_fee_amount)
        .checked_sub(integrator_fee_amount)
        .ok_or(ProgramError::ArithmeticOverflow)?;

    if actual_dst_amount > estimated_dst_amount {
        protocol_fee_amount += (actual_dst_amount - estimated_dst_amount)
            .mul_div_floor(surplus_percentage as u64, BASE_1E2)
            .ok_or(ProgramError::ArithmeticOverflow)?;
    }

    Ok((
        protocol_fee_amount,
        integrator_fee_amount,
        dst_amount - integrator_fee_amount - protocol_fee_amount,
    ))
}

fn build_order_from_reduced(
    order: &ReducedOrderConfig,
    src_mint: Pubkey,
    dst_mint: Pubkey,
    receiver: Pubkey,
    protocol_dst_ata: Option<Pubkey>,
    integrator_dst_ata: Option<Pubkey>,
) -> OrderConfig {
    OrderConfig {
        id: order.id,
        src_amount: order.src_amount,
        min_dst_amount: order.min_dst_amount,
        estimated_dst_amount: order.estimated_dst_amount,
        expiration_time: order.expiration_time,
        native_dst_asset: order.native_dst_asset,
        receiver,
        fee: FeeConfig {
            protocol_dst_ata,
            integrator_dst_ata,
            protocol_fee: order.fee.protocol_fee,
            integrator_fee: order.fee.integrator_fee,
            surplus_percentage: order.fee.surplus_percentage,
            min_cancellation_premium: order.fee.min_cancellation_premium,
            max_cancellation_multiplier: order.fee.max_cancellation_multiplier,
        },
        dutch_auction_data: order.dutch_auction_data.clone(),
        cancellation_auction_duration: order.cancellation_auction_duration,
        src_mint,
        dst_mint,
    }
}
