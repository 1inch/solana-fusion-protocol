use anchor_lang::prelude::*;

use crate::error::EscrowError;

/// Max amount of points in the dutch auction data
pub const MAX_AUCTION_POINTS: usize = 5;

#[derive(AnchorSerialize, AnchorDeserialize, Clone, InitSpace)]
pub struct PointAndTimeDelta {
    rate_bump: u16,
    time_delta: u16,
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, InitSpace)]
pub struct DutchAuctionData {
    pub auction_start_time: u32,
    pub auction_finish_time: u32,
    pub initial_rate_bump: u16,
    #[max_len(MAX_AUCTION_POINTS)]
    pub points_and_time_deltas: Vec<PointAndTimeDelta>,
}

pub fn calculate_rate_bump(timestamp: u64, data: &DutchAuctionData) -> Result<u64> {
    if timestamp <= data.auction_start_time as u64 {
        return Ok(data.initial_rate_bump as u64);
    }
    let auction_finish_time = data.auction_finish_time as u64;
    if timestamp >= auction_finish_time {
        return Ok(0);
    }

    let mut current_rate_bump = data.initial_rate_bump as u64;
    let mut current_point_time = data.auction_start_time as u64;

    for point_and_time_delta in data.points_and_time_deltas.iter() {
        let next_rate_bump = point_and_time_delta.rate_bump as u64;
        let point_time_delta = point_and_time_delta.time_delta as u64;
        let next_point_time = current_point_time
            .checked_add(point_time_delta)
            .ok_or(EscrowError::IntegerOverflow)?;

        if timestamp <= next_point_time {
            return Ok(timestamp
                .checked_sub(current_point_time)
                .ok_or(EscrowError::IntegerOverflow)?
                .checked_mul(next_rate_bump)
                .ok_or(EscrowError::IntegerOverflow)?
                .checked_add(
                    next_point_time
                        .checked_sub(timestamp)
                        .ok_or(EscrowError::IntegerOverflow)?
                        .checked_mul(current_rate_bump)
                        .ok_or(EscrowError::IntegerOverflow)?,
                )
                .ok_or(EscrowError::IntegerOverflow)?
                .checked_div(point_time_delta)
                .ok_or(EscrowError::IntegerOverflow)?);
        }

        current_rate_bump = next_rate_bump;
        current_point_time = next_point_time;
    }
    // Initial check `timestamp >= auction_finish_time` and valid data generation ensure the subtraction in denominator is never zero
    Ok(auction_finish_time
        .checked_sub(timestamp)
        .ok_or(EscrowError::IntegerOverflow)?
        .checked_mul(current_rate_bump)
        .ok_or(EscrowError::IntegerOverflow)?
        .div_ceil(
            auction_finish_time
                .checked_sub(current_point_time)
                .ok_or(EscrowError::IntegerOverflow)?,
        ))
}
