use anchor_lang::prelude::*;
use anchor_spl::associated_token::AssociatedToken;
use anchor_spl::token::{Mint, Token, TokenAccount};
use borsh::BorshSerialize;

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
        order_id: u32,
        expiration_time: u32, // Order expiration time, unix timestamp
        x_amount: u64,        // Amount of tokens maker wants to sell
        y_amount: u64,        // Amount of tokens maker wants in exchange
        escrow_traits: u8,
        taking_amount_getter_program: Option<Pubkey>,
        making_amount_getter_program: Option<Pubkey>,
        predicate_program: Option<Pubkey>,
        extension_hash: Option<[u8; 16]>,
        sol_receiver: Pubkey, // Address to receive SOL when escrow is closed
        receiver: Pubkey,     // Owner of the account which will receive y_token
    ) -> Result<()> {
        let escrow = &mut ctx.accounts.escrow;

        let clock: Clock = Clock::get()?;
        if (expiration_time as i64) < clock.unix_timestamp {
            return err!(EscrowError::OrderExpired);
        }

        let order_invalidator = &mut ctx.accounts.order_invalidator;
        // Find the index of the order id in the invalidator array
        let idx = order_id % constants::INVALIDATOR_SIZE as u32;
        // Check that the order id hasn't been used before or has already expired
        if order_invalidator.invalidator[idx as usize] as i64 >= clock.unix_timestamp {
            return err!(EscrowError::OrderIdAlreadyUsed);
        }
        order_invalidator.invalidator[idx as usize] = expiration_time;

        escrow.set_inner(Escrow {
            x_amount,                          // Amount of tokens maker wants to sell
            x_remaining: x_amount,             // Remaining amount to be filled
            y_amount,                          // Amount of tokens maker wants in exchange
            y_mint: ctx.accounts.y_mint.key(), // token maker wants in exchange
            authorized_user: ctx.accounts.authorized_user.as_ref().map(|acc| acc.key()),
            expiration_time,
            traits: escrow_traits,
            taking_amount_getter_program,
            making_amount_getter_program,
            extension_hash,
            predicate_program,
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

    pub fn accept(
        ctx: Context<Accept>,
        order_id: u32,
        is_x_amount: bool, // True if `amount` represents the asset that taker wants to get, false if it's the asset to give
        amount: u64,
        get_amount_extra_data: Option<Vec<u8>>,
        predicate_extra_data: Option<Vec<u8>>,
    ) -> Result<()> {
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

        let extension_hash =
            utils::calculate_extension_hash(&get_amount_extra_data, &predicate_extra_data);

        // Check that extension hash is the same as the one set during escrow initialization
        if extension_hash != ctx.accounts.escrow.extension_hash {
            return err!(EscrowError::InvalidExtension);
        }

        // Check that getter program is set if needed
        let (expected_getter, provided_getter) = if is_x_amount {
            (
                ctx.accounts.escrow.taking_amount_getter_program,
                ctx.accounts
                    .taking_amount_getter_program
                    .as_ref()
                    .map(|acc| acc.key()),
            )
        } else {
            (
                ctx.accounts.escrow.making_amount_getter_program,
                ctx.accounts
                    .making_amount_getter_program
                    .as_ref()
                    .map(|acc| acc.key()),
            )
        };

        // Check that getter program address which was set during escrow initialization
        // corresponds to the provided program.
        if expected_getter != provided_getter {
            return err!(EscrowError::UnexpectedGetterProgram);
        }

        // Calculate x_amount and y_amount
        let (x_amount, y_amount) = if is_x_amount {
            (
                amount,
                utils::get_y_amount_with_getter(
                    ctx.accounts.escrow.x_amount,
                    ctx.accounts.escrow.y_amount,
                    amount,
                    provided_getter,
                    &get_amount_extra_data,
                ),
            )
        } else {
            let mut y_amount_tmp = amount;
            let mut x_amount_tmp = utils::get_x_amount_with_getter(
                ctx.accounts.escrow.x_amount,
                ctx.accounts.escrow.y_amount,
                amount,
                provided_getter,
                &get_amount_extra_data,
            );
            if x_amount_tmp > ctx.accounts.escrow.x_remaining {
                // Check that taking_amount_getter program address which was set during escrow initialization
                // corresponds to the provided program
                if ctx.accounts.escrow.taking_amount_getter_program
                    != ctx
                        .accounts
                        .taking_amount_getter_program
                        .as_ref()
                        .map(|acc| acc.key())
                {
                    return err!(EscrowError::UnexpectedGetterProgram);
                }
                // Try to decrease y_amount because computed x_amount exceeds remaining amount
                x_amount_tmp = ctx.accounts.escrow.x_remaining;
                y_amount_tmp = utils::get_y_amount_with_getter(
                    ctx.accounts.escrow.x_amount,
                    ctx.accounts.escrow.y_amount,
                    x_amount_tmp,
                    ctx.accounts.escrow.taking_amount_getter_program,
                    &get_amount_extra_data,
                );
                if y_amount_tmp > amount {
                    return err!(EscrowError::YAmountExceeded);
                }
            }

            (x_amount_tmp, y_amount_tmp)
        };

        if ctx.accounts.escrow.x_remaining < x_amount {
            return err!(EscrowError::NotEnoughTokensInEscrow);
        }

        // Check if partial fills are allowed if this is the case
        if !utils::allow_partial_fills(ctx.accounts.escrow.traits)
            && ctx.accounts.escrowed_x_tokens.amount > x_amount
        {
            return err!(EscrowError::PartialFillNotAllowed);
        }

        let expected_predicate = ctx.accounts.escrow.predicate_program;
        let provided_predicate = ctx.accounts.predicate_program.as_ref().map(|acc| acc.key());
        // Check that predicate program address is the same as the one set during escrow initialization
        if expected_predicate != provided_predicate {
            return err!(EscrowError::UnexpectedPredicateProgram);
        }

        // Call predicate program if it's set
        if let Some(predicate_program) = ctx.accounts.escrow.predicate_program {
            let success = bool::try_from_slice(&utils::call_program(
                predicate_program,
                "check_predicate",
                PredicateArgs {
                    taker: ctx.accounts.taker.key(),
                    extra_data: predicate_extra_data,
                },
            )?)?;

            if !success {
                return err!(EscrowError::PredicateNotSatisfied);
            }
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
            x_amount,
        )?;

        // Update x_remaining
        ctx.accounts.escrow.x_remaining -= x_amount;

        // Check that owner of the account which will receive y_token is the same as the one set during escrow initialization
        if ctx.accounts.maker_receiver.key() != ctx.accounts.escrow.receiver {
            return err!(EscrowError::SellerReceiverMismatch);
        }

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
                ctx.accounts.escrowed_x_tokens.amount - x_amount,
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

    /// Pays for the creation of escrow account, order invalidator account and escrow x tokens ATA
    #[account(mut)]
    payer: Signer<'info>,

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
        payer = payer,
        space = constants::DISCRIMINATOR + Escrow::INIT_SPACE,
        seeds = ["escrow6".as_bytes(), maker.key().as_ref(), order_id.to_be_bytes().as_ref()],
        bump,
    )]
    pub escrow: Box<Account<'info, Escrow>>,

    /// Account to store invalidation time of orders
    #[account(
        init_if_needed,
        payer = payer,
        space = constants::DISCRIMINATOR + OrderInvalidator::INIT_SPACE,
        // One OrderInvalidator account for INVALIDATOR_SIZE orders
        seeds = ["order_invalidator".as_bytes(), maker.key().as_ref(), (order_id / constants::INVALIDATOR_SIZE as u32).to_be_bytes().as_ref()],
        bump,
    )]
    order_invalidator: Box<Account<'info, OrderInvalidator>>,

    /// ATA of x_mint to store escrowed tokens
    #[account(
        init,
        payer = payer,
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

    pub taking_amount_getter_program: Option<AccountInfo<'info>>,

    pub making_amount_getter_program: Option<AccountInfo<'info>>,

    pub predicate_program: Option<AccountInfo<'info>>,

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
    // TODO move these pubkeys to 'extension_hash' to do it as it's done in Solidity implementation
    receiver: Pubkey,
    taking_amount_getter_program: Option<Pubkey>,
    making_amount_getter_program: Option<Pubkey>,
    predicate_program: Option<Pubkey>,
    extension_hash: Option<[u8; 16]>,
    sol_receiver: Pubkey,
}

#[account]
#[derive(InitSpace)]
pub struct OrderInvalidator {
    invalidator: [u32; constants::INVALIDATOR_SIZE],
}

#[derive(BorshSerialize)]
pub struct PredicateArgs {
    pub taker: Pubkey,
    pub extra_data: Option<Vec<u8>>,
}
