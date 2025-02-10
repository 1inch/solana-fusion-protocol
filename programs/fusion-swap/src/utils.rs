use anchor_lang::prelude::*;

use crate::constants::BASE_POINTS;

// Function to get amount of `dst_mint` tokens that the taker should pay to the maker using the dutch auction formula
pub fn get_dst_amount(
    escrow_src_amount: u64,
    escrow_dst_amount: u64,
    swap_amount: u64,
    opt_data: Option<DutchAuctionData>,
) -> Result<u64> {
    let mut result = (escrow_dst_amount * swap_amount).div_ceil(escrow_src_amount);
    if let Some(data) = opt_data {
        let rate_bump = calculate_rate_bump(
            Clock::get()?.unix_timestamp as u64,
            data.auction_start_time as u64,
            data.auction_finish_time as u64,
            data.initial_rate_bump as u64,
        );

        result = (result * (BASE_POINTS + rate_bump)).div_ceil(BASE_POINTS);
    }
    Ok(result)
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
