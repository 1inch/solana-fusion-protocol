use anchor_lang::prelude::*;
use anchor_spl::{
    associated_token::AssociatedToken,
    token::spl_token::native_mint,
    token_interface::{
        close_account, transfer_checked, CloseAccount, Mint, TokenAccount, TokenInterface,
        TransferChecked,
    },
};
use common::constants::DISCRIMINATOR;
use dutch_auction::{calculate_rate_bump, DutchAuctionData};
use muldiv::MulDiv;

pub mod dutch_auction;
pub mod error;

use error::EscrowError;

declare_id!("9hbsrgqQUYBPdAiriyn5A7cr3zBzN3EmeXN6mJLyizHh");

pub const BASE_1E2: u64 = 100;
pub const BASE_1E5: u64 = 100_000;

#[program]
pub mod fusion_swap {
    use super::*;

    pub fn create(ctx: Context<Create>, order: OrderConfig) -> Result<()> {
        require!(
            order.src_amount != 0 && order.min_dst_amount != 0,
            EscrowError::InvalidAmount
        );

        // we support only original spl_token::native_mint
        require!(
            ctx.accounts.dst_mint.key() == native_mint::id() || !order.native_dst_asset,
            EscrowError::InconsistentNativeDstTrait
        );

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
            && ctx.accounts.protocol_dst_ata.is_none())
            || (order.fee.protocol_fee == 0
                && order.fee.surplus_percentage == 0
                && ctx.accounts.protocol_dst_ata.is_some())
        {
            return Err(EscrowError::InconsistentProtocolFeeConfig.into());
        }

        if (order.fee.integrator_fee > 0 && ctx.accounts.integrator_dst_ata.is_none())
            || (order.fee.integrator_fee == 0 && ctx.accounts.integrator_dst_ata.is_some())
        {
            return Err(EscrowError::InconsistentIntegratorFeeConfig.into());
        }

        let escrow = &mut ctx.accounts.escrow;
        escrow.set_inner(Escrow {
            src_remaining: order.src_amount,
            // min_dst_amount: order.min_dst_amount,
            // expiration_time: order.expiration_time,
            // native_dst_asset: order.native_dst_asset,
            // receiver: order.receiver,
            // fee: order.fee,
            // protocol_dst_ata: ctx.accounts.protocol_dst_ata.as_ref().map(|acc| acc.key()),
            // integrator_dst_ata: ctx
            //     .accounts
            //     .integrator_dst_ata
            //     .as_ref()
            //     .map(|acc| acc.key()),
            // estimated_dst_amount: order.estimated_dst_amount,
            // dutch_auction_data: order.dutch_auction_data,
        });

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

    pub fn fill(ctx: Context<Fill>, order: OrderConfig, amount: u64) -> Result<()> {
        require!(
            Clock::get()?.unix_timestamp <= order.expiration_time as i64,
            EscrowError::OrderExpired
        );

        require!(
            amount <= ctx.accounts.escrow.src_remaining,
            EscrowError::NotEnoughTokensInEscrow
        );

        require!(amount != 0, EscrowError::InvalidAmount);

        // Update src_remaining
        ctx.accounts.escrow.src_remaining -= amount;

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
                    order.id.to_be_bytes().as_ref(),
                    ctx.accounts.src_mint.key().as_ref(),
                    ctx.accounts.dst_mint.key().as_ref(),
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
            order.fee.integrator_fee as u64,
            order.fee.protocol_fee as u64,
            order.fee.surplus_percentage as u64,
            dst_amount,
            get_dst_amount(
                order.src_amount,
                order.estimated_dst_amount,
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
                .ok_or(EscrowError::InconsistentIntegratorFeeConfig)?;

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
                .ok_or(EscrowError::MissingMakerDstAta)?;

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
        if ctx.accounts.escrow.src_remaining == 0 {
            close_escrow(
                ctx.accounts.src_token_program.to_account_info(),
                &ctx.accounts.escrow,
                ctx.accounts.escrow_src_ata.to_account_info(),
                ctx.accounts.maker.to_account_info(),
                ctx.accounts.src_mint.to_account_info(),
                ctx.accounts.dst_mint.key(),
                order.id,
                ctx.bumps.escrow,
            )?;
        }

        Ok(())
    }

    pub fn cancel(
        ctx: Context<Cancel>,
        dst_mint_key: Pubkey,
        order: OrderConfig,
        protocol_dst_ata: Option<Pubkey>,
        integrator_dst_ata: Option<Pubkey>,
    ) -> Result<()> {
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
                    order.id.to_be_bytes().as_ref(),
                    ctx.accounts.src_mint.key().as_ref(),
                    dst_mint_key.as_ref(),
                    &[ctx.bumps.escrow],
                ]],
            ),
            ctx.accounts.escrow_src_ata.amount,
            ctx.accounts.src_mint.decimals,
        )?;

        close_escrow(
            ctx.accounts.src_token_program.to_account_info(),
            &ctx.accounts.escrow,
            ctx.accounts.escrow_src_ata.to_account_info(),
            ctx.accounts.maker.to_account_info(),
            ctx.accounts.src_mint.to_account_info(),
            dst_mint_key,
            order.id,
            ctx.bumps.escrow,
        )
    }
}

#[derive(Accounts)]
#[instruction(order: OrderConfig)]
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

    /// Account to store order conditions
    #[account(
        init,
        payer = maker,
        space = DISCRIMINATOR + Escrow::INIT_SPACE,
        seeds = [
            "escrow".as_bytes(),
            maker.key().as_ref(),
            order.id.to_be_bytes().as_ref(),
            src_mint.key().as_ref(),
            dst_mint.key().as_ref(),
            order.src_amount.to_be_bytes().as_ref(),
            order.min_dst_amount.to_be_bytes().as_ref(),
            order.estimated_dst_amount.to_be_bytes().as_ref(),
            order.expiration_time.to_be_bytes().as_ref(),
            &[order.native_dst_asset as u8],
            order.receiver.as_ref(),
            order.fee.protocol_fee.to_be_bytes().as_ref(),
            order.fee.integrator_fee.to_be_bytes().as_ref(),
            order.fee.surplus_percentage.to_be_bytes().as_ref(),
            order.dutch_auction_data.start_time.to_be_bytes().as_ref(),
            order.dutch_auction_data.duration.to_be_bytes().as_ref(),
            order.dutch_auction_data.initial_rate_bump.to_be_bytes().as_ref(),
            // TODO: points?
            if let Some(x) = protocol_dst_ata {
                x.key().as_ref()
            } else {
                &[]
            },
            // integrator_dst_ata,
        ],
        bump,
    )]
    escrow: Box<Account<'info, Escrow>>,

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
        constraint = protocol_dst_ata.mint == dst_mint.key() @ EscrowError::InconsistentProtocolFeeConfig
    )]
    protocol_dst_ata: Option<Box<InterfaceAccount<'info, TokenAccount>>>,

    #[account(
        constraint = integrator_dst_ata.mint == dst_mint.key() @ EscrowError::InconsistentIntegratorFeeConfig
    )]
    integrator_dst_ata: Option<Box<InterfaceAccount<'info, TokenAccount>>>,

    associated_token_program: Program<'info, AssociatedToken>,
    src_token_program: Interface<'info, TokenInterface>,
    system_program: Program<'info, System>,
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
        bump,
        seeds::program = whitelist::ID,
    )]
    resolver_access: Account<'info, whitelist::ResolverAccess>,

    /// CHECK: check is not necessary as maker is not spending any funds
    #[account(mut)]
    maker: AccountInfo<'info>,

    /// CHECK: maker_receiver only has to be equal to escrow parameter
    #[account(
        constraint = order.receiver == maker_receiver.key() @ EscrowError::SellerReceiverMismatch,
    )]
    maker_receiver: AccountInfo<'info>,

    /// Maker asset
    src_mint: Box<InterfaceAccount<'info, Mint>>,
    /// Taker asset
    dst_mint: Box<InterfaceAccount<'info, Mint>>,

    /// Account to store order conditions
    #[account(
        mut,
        seeds = [
            "escrow".as_bytes(),
            maker.key().as_ref(),
            order.id.to_be_bytes().as_ref(),
            src_mint.key().as_ref(),
            dst_mint.key().as_ref(),
            order.src_amount.to_be_bytes().as_ref(),
            order.min_dst_amount.to_be_bytes().as_ref(),
            order.estimated_dst_amount.to_be_bytes().as_ref(),
            order.expiration_time.to_be_bytes().as_ref(),
            &[order.native_dst_asset as u8],
            order.receiver.as_ref(),
            order.fee.protocol_fee.to_be_bytes().as_ref(),
            order.fee.integrator_fee.to_be_bytes().as_ref(),
            order.fee.surplus_percentage.to_be_bytes().as_ref(),
            order.dutch_auction_data.start_time.to_be_bytes().as_ref(),
            order.dutch_auction_data.duration.to_be_bytes().as_ref(),
            order.dutch_auction_data.initial_rate_bump.to_be_bytes().as_ref(),
            // TODO: points?
            protocol_dst_ata,
            integrator_dst_ata,
        ],
        bump,
    )]
    escrow: Box<Account<'info, Escrow>>,

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

    protocol_dst_ata: Option<Box<InterfaceAccount<'info, TokenAccount>>>,

    integrator_dst_ata: Option<Box<InterfaceAccount<'info, TokenAccount>>>,

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
#[instruction(order: OrderConfig, dst_mint_key: Pubkey, protocol_dst_ata: Option<Pubkey>, integrator_dst_ata: Option<Pubkey>)]
pub struct Cancel<'info> {
    /// Account that created the escrow
    #[account(mut, signer)]
    maker: Signer<'info>,

    /// Maker asset
    src_mint: InterfaceAccount<'info, Mint>,

    /// Account to store order conditions
    #[account(
        mut,
        seeds = [
            "escrow".as_bytes(),
            maker.key().as_ref(),
            order.id.to_be_bytes().as_ref(),
            src_mint.key().as_ref(),
            dst_mint_key.as_ref(),
            order.src_amount.to_be_bytes().as_ref(),
            order.min_dst_amount.to_be_bytes().as_ref(),
            order.estimated_dst_amount.to_be_bytes().as_ref(),
            order.expiration_time.to_be_bytes().as_ref(),
            &[order.native_dst_asset as u8],
            order.receiver.as_ref(),
            order.fee.protocol_fee.to_be_bytes().as_ref(),
            order.fee.integrator_fee.to_be_bytes().as_ref(),
            order.fee.surplus_percentage.to_be_bytes().as_ref(),
            order.dutch_auction_data.start_time.to_be_bytes().as_ref(),
            order.dutch_auction_data.duration.to_be_bytes().as_ref(),
            order.dutch_auction_data.initial_rate_bump.to_be_bytes().as_ref(),
            // TODO: points?
            protocol_dst_ata,
            integrator_dst_ata,
        ],
        bump,
    )]
    escrow: Box<Account<'info, Escrow>>,

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
    dutch_auction_data: DutchAuctionData,
}

/// Core data structure for an escrow
#[account]
#[derive(InitSpace)]
pub struct Escrow {
    // /// Amount of `src_mint` tokens the maker is offering to sell
    // /// The `src_mint` token is not stored in Escrow; it is referenced from `Create` via `src_mint` account.
    // src_amount: u64,

    /// Remaining amount of `src_mint` tokens available for fill
    /// This field does not affect the created escrow in the `create` method, as it is always overwritten with the `src_amount` value.
    src_remaining: u64,

    // /// Minimum amount of `dst_mint` tokens the maker wants to receive
    // min_dst_amount: u64,

    // /// Estimated amount of `dst_mint` tokens the maker expects to receive.
    // estimated_dst_amount: u64,

    // /// Unix timestamp indicating when the escrow expires
    // expiration_time: u32,

    // /// Flag indicates whether `dst_mint` is native SOL (`true`) or an SPL token (`false`)
    // native_dst_asset: bool,

    // /// The wallet which will receive the `dst_mint` tokens
    // receiver: Pubkey,

    // /// See {FeeConfig}
    // fee: FeeConfig,

    // /// Associated token account for collecting protocol fees
    // protocol_dst_ata: Option<Pubkey>,

    // /// Associated token account for collecting integrator fees
    // integrator_dst_ata: Option<Pubkey>,

    // /// Dutch auction parameters defining price adjustments over time
    // dutch_auction_data: DutchAuctionData,
}

// Function to close the escrow account
fn close_escrow<'info>(
    token_program: AccountInfo<'info>,
    escrow: &Account<'info, Escrow>,
    escrow_src_ata: AccountInfo<'info>,
    maker: AccountInfo<'info>,
    src_mint: AccountInfo<'info>,
    dst_mint_key: Pubkey,
    order_id: u32,
    escrow_bump: u8,
) -> Result<()> {
    // Close escrow_src_ata account
    close_account(CpiContext::new_with_signer(
        token_program,
        CloseAccount {
            account: escrow_src_ata,
            destination: maker.to_account_info(),
            authority: escrow.to_account_info(),
        },
        &[&[
            "escrow".as_bytes(),
            maker.key().as_ref(),
            order_id.to_be_bytes().as_ref(),
            src_mint.key().as_ref(),
            dst_mint_key.as_ref(),
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
    integrator_fee: u64,
    protocol_fee: u64,
    surplus_percentage: u64,
    dst_amount: u64,
    estimated_dst_amount: u64,
) -> Result<(u64, u64, u64)> {
    let integrator_fee_amount = dst_amount
        .mul_div_floor(integrator_fee, BASE_1E5)
        .ok_or(ProgramError::ArithmeticOverflow)?;

    let mut protocol_fee_amount = dst_amount
        .mul_div_floor(protocol_fee, BASE_1E5)
        .ok_or(ProgramError::ArithmeticOverflow)?;

    let actual_dst_amount = (dst_amount - protocol_fee_amount)
        .checked_sub(integrator_fee_amount)
        .ok_or(ProgramError::ArithmeticOverflow)?;

    if actual_dst_amount > estimated_dst_amount {
        protocol_fee_amount += (actual_dst_amount - estimated_dst_amount)
            .mul_div_floor(surplus_percentage, BASE_1E2)
            .ok_or(ProgramError::ArithmeticOverflow)?;
    }

    Ok((
        protocol_fee_amount,
        integrator_fee_amount,
        dst_amount - protocol_fee_amount - integrator_fee_amount,
    ))
}
