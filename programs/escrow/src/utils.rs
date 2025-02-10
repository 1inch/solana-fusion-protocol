use anchor_lang::prelude::*;
use anchor_lang::solana_program::system_program;

use crate::constants::BASE_POINTS;
use crate::error::EscrowError;

// Flag that defines if the order can be filled partially
pub fn allow_partial_fills(traits: u8) -> bool {
    traits & 0b00000001 != 0
}

// Flag that defines if the order can be filled multiple times
pub fn allow_multiple_fills(traits: u8) -> bool {
    traits & 0b00000010 != 0
}

// Function to close the escrow account
pub fn close<'info>(
    token_program: AccountInfo<'info>,
    escrow: AccountInfo<'info>,
    escrowed_x_tokens: AccountInfo<'info>,
    escrowed_x_tokens_amount: u64,
    maker_x_token: AccountInfo<'info>,
    maker: AccountInfo<'info>,
    sol_receiver: AccountInfo<'info>,
    order_id: u32,
    escrow_bump: u8,
) -> Result<()> {
    // return maker's x_token back to account
    if escrowed_x_tokens_amount > 0 {
        anchor_spl::token::transfer(
            CpiContext::new_with_signer(
                token_program.clone(),
                anchor_spl::token::Transfer {
                    from: escrowed_x_tokens.to_account_info(),
                    to: maker_x_token,
                    authority: escrow.clone(),
                },
                &[&[
                    "escrow6".as_bytes(),
                    maker.key().as_ref(),
                    order_id.to_be_bytes().as_ref(),
                    &[escrow_bump],
                ]],
            ),
            escrowed_x_tokens_amount,
        )?;
    }

    // Close escrowed_x_tokens account
    anchor_spl::token::close_account(CpiContext::new_with_signer(
        token_program.clone(),
        anchor_spl::token::CloseAccount {
            account: escrowed_x_tokens.to_account_info(),
            destination: sol_receiver.to_account_info(),
            authority: escrow.clone(),
        },
        &[&[
            "escrow6".as_bytes(),
            maker.key().as_ref(),
            order_id.to_be_bytes().as_ref(),
            &[escrow_bump],
        ]],
    ))?;

    // Close escrow account
    close_account(escrow, sol_receiver)?;

    Ok(())
}

pub fn close_account<'info>(
    info: AccountInfo<'info>,
    sol_destination: AccountInfo<'info>,
) -> Result<()> {
    // Transfer tokens from the account to the sol_destination.
    let dest_starting_lamports = sol_destination.lamports();
    **sol_destination.lamports.borrow_mut() =
        dest_starting_lamports.checked_add(info.lamports()).unwrap();
    **info.lamports.borrow_mut() = 0;

    info.assign(&system_program::ID);
    info.realloc(0, false).map_err(Into::into)
}

// Function to get amount of `y_mint` tokens that the taker should pay to the maker using the default formula
pub fn get_y_amount(escrow_x_amount: u64, escrow_y_amount: u64, swap_amount: u64, opt_data: Option<DutchAuctionData>) -> Result<u64> {
    if let Some(data) = opt_data {
        let rate_bump = calculate_rate_bump(
            Clock::get()?.unix_timestamp as u64,
            data.auction_start_time as u64,
            data.auction_finish_time as u64,
            data.initial_rate_bump as u64,
        );

        let result = (escrow_y_amount * swap_amount).div_ceil(escrow_x_amount);
        let result = (result * (BASE_POINTS + rate_bump)).div_ceil(BASE_POINTS);
        Ok(result)
    } else {
        Err(EscrowError::DutchAuctionDataNotProvided.into())
    }
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, InitSpace)]
pub struct PointsAndTimeDeltas {
    rate_bump: u32,
    point_time: u16,
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, InitSpace)]
pub struct DutchAuctionData {
    pub auction_start_time: u32,
    pub auction_finish_time: u32,
    pub initial_rate_bump: u32,
    #[max_len(5)]
    pub points_and_time_deltas: Vec<PointsAndTimeDeltas>,
}

pub fn calculate_rate_bump(
    cur_timestamp: u64,
    auction_start_time: u64,
    auction_finish_time: u64,
    initial_rate_bump: u64,
) -> u64 {
    if cur_timestamp <= auction_start_time {
        initial_rate_bump
    } else if cur_timestamp < auction_finish_time {
        (auction_finish_time - cur_timestamp) * initial_rate_bump
            / (auction_finish_time - auction_start_time)
    } else {
        0
    }
}
