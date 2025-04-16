use anchor_lang::solana_program::hash::hashv;
use anchor_lang::{prelude::*, system_program};
use anchor_spl::{
    associated_token::AssociatedToken,
    token::spl_token::native_mint,
    token_interface::{
        close_account, transfer_checked, CloseAccount, Mint, TokenAccount, TokenInterface,
        TransferChecked,
    },
};
use auction::{calculate_premium, calculate_rate_bump, AuctionData};
use common::constants::*;
use muldiv::MulDiv;

pub mod auction;
pub mod error;

use error::FusionError;

declare_id!("5uzpYuGqBaetRMXPDtGWGN9W4mdmgBzpGHcQACrZ1npi");

enum UniTransferParams<'info> {
    NativeTransfer {
        from: AccountInfo<'info>,
        to: AccountInfo<'info>,
        amount: u64,
        program: Program<'info, System>,
    },

    TokenTransfer {
        from: AccountInfo<'info>,
        authority: AccountInfo<'info>,
        to: AccountInfo<'info>,
        mint: InterfaceAccount<'info, Mint>,
        amount: u64,
        program: Interface<'info, TokenInterface>,
    },
}

#[program]
pub mod fusion_swap {
    use super::*;

    pub fn create(ctx: Context<Create>, order: OrderConfig) -> Result<()> {
        require!(
            order.src_amount != 0 && order.min_dst_amount != 0,
            FusionError::InvalidAmount
        );

        // we support only original spl_token::native_mint
        require!(
            ctx.accounts.src_mint.key() == native_mint::id() || !order.src_asset_is_native,
            FusionError::InconsistentNativeSrcTrait
        );

        require!(
            ctx.accounts.dst_mint.key() == native_mint::id() || !order.dst_asset_is_native,
            FusionError::InconsistentNativeDstTrait
        );

        require!(
            Clock::get()?.unix_timestamp < order.expiration_time as i64,
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

        // Iff protocol fee or surplus is positive, protocol_dst_acc must be set
        require!(
            (order.fee.protocol_fee > 0 || order.fee.surplus_percentage > 0)
                == ctx.accounts.protocol_dst_acc.is_some(),
            FusionError::InconsistentProtocolFeeConfig
        );

        // Iff integrator fee is positive, integrator_dst_acc must be set
        require!(
            (order.fee.integrator_fee > 0) == ctx.accounts.integrator_dst_acc.is_some(),
            FusionError::InconsistentIntegratorFeeConfig
        );

        require!(
            ctx.accounts.escrow_src_ata.to_account_info().lamports()
                >= order.fee.max_cancellation_premium,
            FusionError::InvalidCancellationFee
        );

        // Maker => Escrow
        if order.src_asset_is_native {
            // Wrap SOL to wSOL
            uni_transfer(&UniTransferParams::NativeTransfer {
                from: ctx.accounts.maker.to_account_info(),
                to: ctx.accounts.escrow_src_ata.to_account_info(),
                amount: order.src_amount,
                program: ctx.accounts.system_program.clone(),
            })?;

            anchor_spl::token::sync_native(CpiContext::new(
                ctx.accounts.src_token_program.to_account_info(),
                anchor_spl::token::SyncNative {
                    account: ctx.accounts.escrow_src_ata.to_account_info(),
                },
            ))
        } else {
            uni_transfer(&UniTransferParams::TokenTransfer {
                from: ctx.accounts.maker_src_ata.to_account_info(),
                authority: ctx.accounts.maker.to_account_info(),
                to: ctx.accounts.escrow_src_ata.to_account_info(),
                mint: *ctx.accounts.src_mint.clone(),
                amount: order.src_amount,
                program: ctx.accounts.src_token_program.clone(),
            })
        }
    }

    pub fn fill(ctx: Context<Fill>, order: OrderConfig, amount: u64) -> Result<()> {
        require!(
            Clock::get()?.unix_timestamp < order.expiration_time as i64,
            FusionError::OrderExpired
        );

        require!(
            amount <= ctx.accounts.escrow_src_ata.amount,
            FusionError::NotEnoughTokensInEscrow
        );

        require!(amount != 0, FusionError::InvalidAmount);

        let order_src_mint = ctx.accounts.src_mint.key();
        let order_dst_mint = ctx.accounts.dst_mint.key();
        let order_receiver = ctx.accounts.maker_receiver.key();
        let protocol_dst_acc = ctx.accounts.protocol_dst_acc.as_ref().map(|acc| acc.key());
        let integrator_dst_acc = ctx
            .accounts
            .integrator_dst_acc
            .as_ref()
            .map(|acc| acc.key());

        let order_hash = &order_hash(
            &order,
            protocol_dst_acc,
            integrator_dst_acc,
            order_src_mint,
            order_dst_mint,
            order_receiver,
        )?;

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
                    order_hash,
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

        // Taker => Maker
        let mut params = if order.dst_asset_is_native {
            UniTransferParams::NativeTransfer {
                from: ctx.accounts.taker.to_account_info(),
                to: ctx.accounts.maker_receiver.to_account_info(),
                amount: maker_dst_amount,
                program: ctx.accounts.system_program.clone(),
            }
        } else {
            UniTransferParams::TokenTransfer {
                from: ctx
                    .accounts
                    .taker_dst_ata
                    .as_ref()
                    .ok_or(FusionError::MissingTakerDstAta)?
                    .to_account_info(),
                authority: ctx.accounts.taker.to_account_info(),
                to: ctx
                    .accounts
                    .maker_dst_ata
                    .as_ref()
                    .ok_or(FusionError::MissingMakerDstAta)?
                    .to_account_info(),
                mint: *ctx.accounts.dst_mint.clone(),
                amount: maker_dst_amount,
                program: ctx.accounts.dst_token_program.clone(),
            }
        };
        uni_transfer(&params)?;

        // Take protocol fee
        if protocol_fee_amount > 0 {
            match &mut params {
                UniTransferParams::NativeTransfer { amount, to, .. }
                | UniTransferParams::TokenTransfer { amount, to, .. } => {
                    *amount = protocol_fee_amount;
                    *to = ctx
                        .accounts
                        .protocol_dst_acc
                        .as_ref()
                        .ok_or(FusionError::InconsistentProtocolFeeConfig)?
                        .to_account_info();
                }
            }
            uni_transfer(&params)?;
        }

        // Take integrator fee
        if integrator_fee_amount > 0 {
            match &mut params {
                UniTransferParams::NativeTransfer { amount, to, .. }
                | UniTransferParams::TokenTransfer { amount, to, .. } => {
                    *amount = integrator_fee_amount;
                    *to = ctx
                        .accounts
                        .integrator_dst_acc
                        .as_ref()
                        .ok_or(FusionError::InconsistentProtocolFeeConfig)?
                        .to_account_info();
                }
            }
            uni_transfer(&params)?;
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
                    order_hash,
                    &[ctx.bumps.escrow],
                ]],
            ))?;
        }

        Ok(())
    }

    pub fn cancel(
        ctx: Context<Cancel>,
        order_hash: [u8; 32],
        order_src_asset_is_native: bool,
    ) -> Result<()> {
        require!(
            ctx.accounts.src_mint.key() == native_mint::id() || !order_src_asset_is_native,
            FusionError::InconsistentNativeSrcTrait
        );

        // Return remaining src tokens back to maker
        if !order_src_asset_is_native {
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

    pub fn cancel_by_resolver(
        ctx: Context<CancelByResolver>,
        order: OrderConfig,
        reward_limit: u64,
    ) -> Result<()> {
        require!(
            order.fee.max_cancellation_premium > 0,
            FusionError::CancelOrderByResolverIsForbidden
        );
        let current_timestamp = Clock::get()?.unix_timestamp;
        require!(
            current_timestamp >= order.expiration_time as i64,
            FusionError::OrderNotExpired
        );

        let order_hash = order_hash(
            &order,
            ctx.accounts.protocol_dst_acc.as_ref().map(|acc| acc.key()),
            ctx.accounts
                .integrator_dst_acc
                .as_ref()
                .map(|acc| acc.key()),
            ctx.accounts.src_mint.key(),
            ctx.accounts.dst_mint.key(),
            ctx.accounts.maker_receiver.key(),
        )?;

        // Return remaining src tokens back to maker
        if !order.src_asset_is_native {
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
        };

        let cancellation_premium = calculate_premium(
            current_timestamp as u32,
            order.expiration_time,
            order.cancellation_auction_duration,
            order.fee.max_cancellation_premium,
        );
        let maker_amount = ctx.accounts.escrow_src_ata.to_account_info().lamports()
            - std::cmp::min(cancellation_premium, reward_limit);

        // Transfer all the remaining lamports to the resolver first
        close_account(CpiContext::new_with_signer(
            ctx.accounts.src_token_program.to_account_info(),
            CloseAccount {
                account: ctx.accounts.escrow_src_ata.to_account_info(),
                destination: ctx.accounts.resolver.to_account_info(),
                authority: ctx.accounts.escrow.to_account_info(),
            },
            &[&[
                "escrow".as_bytes(),
                ctx.accounts.maker.key().as_ref(),
                &order_hash,
                &[ctx.bumps.escrow],
            ]],
        ))?;

        // Transfer all lamports from the closed account, minus the cancellation premium, to the maker
        uni_transfer(&UniTransferParams::NativeTransfer {
            from: ctx.accounts.resolver.to_account_info(),
            to: ctx.accounts.maker.to_account_info(),
            amount: maker_amount,
            program: ctx.accounts.system_program.clone(),
        })
    }
}

#[derive(Accounts)]
#[instruction(order: OrderConfig)]
pub struct Create<'info> {
    system_program: Program<'info, System>,

    /// Account to store order conditions
    #[account(
        seeds = [
            "escrow".as_bytes(),
            maker.key().as_ref(),
            &order_hash(
                &order,
                protocol_dst_acc.clone().map(|acc| acc.key()),
                integrator_dst_acc.clone().map(|acc| acc.key()),
                src_mint.key(),
                dst_mint.key(),
                maker_receiver.key(),
            )?,
        ],
        bump,
    )]
    /// CHECK: check is not needed here as we never initialize the account
    escrow: UncheckedAccount<'info>,

    /// Source asset
    src_mint: Box<InterfaceAccount<'info, Mint>>,

    src_token_program: Interface<'info, TokenInterface>,

    /// ATA of src_mint to store escrowed tokens
    #[account(
        init,
        payer = maker,
        associated_token::mint = src_mint,
        associated_token::authority = escrow,
        associated_token::token_program = src_token_program,
    )]
    escrow_src_ata: Box<InterfaceAccount<'info, TokenAccount>>,

    /// `maker`, who is willing to sell src token for dst token
    #[account(mut, signer)]
    maker: Signer<'info>,

    /// Maker's ATA of src_mint
    #[account(
        mut,
        associated_token::mint = src_mint,
        associated_token::authority = maker,
        associated_token::token_program = src_token_program,
    )]
    maker_src_ata: Box<InterfaceAccount<'info, TokenAccount>>,

    /// Destination asset
    dst_mint: Box<InterfaceAccount<'info, Mint>>,

    /// CHECK: maker_receiver only has to be equal to escrow parameter
    maker_receiver: UncheckedAccount<'info>,

    associated_token_program: Program<'info, AssociatedToken>,

    protocol_dst_acc: Option<UncheckedAccount<'info>>,

    integrator_dst_acc: Option<UncheckedAccount<'info>>,
}

#[derive(Accounts)]
#[instruction(order: OrderConfig)]
pub struct Fill<'info> {
    /// `taker`, who buys `src_mint` for `dst_mint`
    #[account(mut, signer)]
    taker: Signer<'info>,
    /// Account allowed to fill the order
    #[account(
        seeds = [whitelist::RESOLVER_ACCESS_SEED, taker.key().as_ref()],
        bump = resolver_access.bump,
        seeds::program = whitelist::ID,
    )]
    resolver_access: Account<'info, whitelist::ResolverAccess>,

    /// CHECK: check is not necessary as maker is not spending any funds
    #[account(mut)]
    maker: UncheckedAccount<'info>,

    /// CHECK: maker_receiver only has to be equal to escrow parameter
    #[account(mut)]
    maker_receiver: UncheckedAccount<'info>,

    /// Maker asset
    src_mint: Box<InterfaceAccount<'info, Mint>>,
    /// Taker asset
    dst_mint: Box<InterfaceAccount<'info, Mint>>,

    /// Account to store order conditions
    #[account(
        seeds = [
            "escrow".as_bytes(),
            maker.key().as_ref(),
            &order_hash(
                &order,
                protocol_dst_acc.clone().map(|acc| acc.key()),
                integrator_dst_acc.clone().map(|acc| acc.key()),
                src_mint.key(),
                dst_mint.key(),
                maker_receiver.key(),
            )?,
        ],
        bump,
    )]
    /// CHECK: check is not needed here as we never initialize the account
    escrow: UncheckedAccount<'info>,

    /// ATA of src_mint to store escrowed tokens
    #[account(
        mut,
        associated_token::mint = src_mint,
        associated_token::authority = escrow,
        associated_token::token_program = src_token_program,
    )]
    escrow_src_ata: Box<InterfaceAccount<'info, TokenAccount>>,

    /// Taker's ATA of src_mint
    #[account(
        mut,
        constraint = taker_src_ata.mint.key() == src_mint.key()
    )]
    taker_src_ata: Box<InterfaceAccount<'info, TokenAccount>>,

    src_token_program: Interface<'info, TokenInterface>,
    dst_token_program: Interface<'info, TokenInterface>,
    system_program: Program<'info, System>,
    associated_token_program: Program<'info, AssociatedToken>,

    /// Maker's ATA of dst_mint
    #[account(
        init_if_needed,
        payer = taker,
        associated_token::mint = dst_mint,
        associated_token::authority = maker_receiver,
        associated_token::token_program = dst_token_program,
    )]
    maker_dst_ata: Option<Box<InterfaceAccount<'info, TokenAccount>>>,

    /// Taker's ATA of dst_mint
    #[account(
        mut,
        associated_token::mint = dst_mint,
        associated_token::authority = taker,
        associated_token::token_program = dst_token_program,
    )]
    taker_dst_ata: Option<Box<InterfaceAccount<'info, TokenAccount>>>,

    #[account(mut)]
    protocol_dst_acc: Option<UncheckedAccount<'info>>,

    #[account(mut)]
    integrator_dst_acc: Option<UncheckedAccount<'info>>,
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
    escrow: UncheckedAccount<'info>,

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
#[instruction(order: OrderConfig)]
pub struct CancelByResolver<'info> {
    /// Account that cancels the escrow
    #[account(mut, signer)]
    resolver: Signer<'info>,

    /// Account allowed to cancel the order
    #[account(
        seeds = [whitelist::RESOLVER_ACCESS_SEED, resolver.key().as_ref()],
        bump = resolver_access.bump,
        seeds::program = whitelist::ID,
    )]
    resolver_access: Account<'info, whitelist::ResolverAccess>,

    /// CHECK: check is not necessary as maker is not spending any funds
    #[account(mut)]
    maker: UncheckedAccount<'info>,

    /// CHECK: maker_receiver only has to be equal to escrow parameter
    maker_receiver: UncheckedAccount<'info>,

    /// Maker asset
    src_mint: InterfaceAccount<'info, Mint>,

    /// Taker asset
    dst_mint: Box<InterfaceAccount<'info, Mint>>,

    /// Account to store order conditions
    #[account(
        seeds = [
            "escrow".as_bytes(),
            maker.key().as_ref(),
            &order_hash(
                &order,
                protocol_dst_acc.clone().map(|acc| acc.key()),
                integrator_dst_acc.clone().map(|acc| acc.key()),
                src_mint.key(),
                dst_mint.key(),
                maker_receiver.key(),
            )?,
        ],
        bump,
    )]
    /// CHECK: check is not needed here as we never initialize the account
    escrow: UncheckedAccount<'info>,

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
    system_program: Program<'info, System>,

    protocol_dst_acc: Option<UncheckedAccount<'info>>,

    integrator_dst_acc: Option<UncheckedAccount<'info>>,
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

    /// Maximum cancellation premium
    /// Value in absolute lamports amount
    max_cancellation_premium: u64,
}

#[derive(AnchorSerialize, AnchorDeserialize)]
pub struct OrderConfig {
    id: u32,
    src_amount: u64,
    min_dst_amount: u64,
    estimated_dst_amount: u64,
    expiration_time: u32,
    src_asset_is_native: bool,
    dst_asset_is_native: bool,
    fee: FeeConfig,
    dutch_auction_data: AuctionData,
    cancellation_auction_duration: u32,
}

fn order_hash(
    order: &OrderConfig,
    protocol_dst_acc: Option<Pubkey>,
    integrator_dst_acc: Option<Pubkey>,
    src_mint: Pubkey,
    dst_mint: Pubkey,
    receiver: Pubkey,
) -> Result<[u8; 32]> {
    Ok(hashv(&[
        &order.try_to_vec()?,
        &protocol_dst_acc.try_to_vec()?,
        &integrator_dst_acc.try_to_vec()?,
        &src_mint.to_bytes(),
        &dst_mint.to_bytes(),
        &receiver.to_bytes(),
    ])
    .to_bytes())
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

fn uni_transfer(params: &UniTransferParams<'_>) -> Result<()> {
    match params {
        UniTransferParams::NativeTransfer {
            from,
            to,
            amount,
            program,
        } => system_program::transfer(
            CpiContext::new(
                program.to_account_info(),
                system_program::Transfer {
                    from: from.to_account_info(),
                    to: to.to_account_info(),
                },
            ),
            *amount,
        ),
        UniTransferParams::TokenTransfer {
            from,
            authority,
            to,
            mint,
            amount,
            program,
        } => transfer_checked(
            CpiContext::new(
                program.to_account_info(),
                TransferChecked {
                    from: from.to_account_info(),
                    mint: mint.to_account_info(),
                    to: to.to_account_info(),
                    authority: authority.to_account_info(),
                },
            ),
            *amount,
            mint.decimals,
        ),
    }
}
