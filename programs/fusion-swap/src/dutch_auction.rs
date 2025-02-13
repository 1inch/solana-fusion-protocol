use anchor_lang::prelude::*;

use crate::constants::AUCTION_POINTS;

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
    #[max_len(AUCTION_POINTS)]
    pub points_and_time_deltas: Vec<PointAndTimeDelta>,
}

pub fn calculate_rate_bump(timestamp: u64, data: &DutchAuctionData) -> u64 {
    if timestamp <= data.auction_start_time as u64 {
        return data.initial_rate_bump as u64;
    }
    let auction_finish_time = data.auction_finish_time as u64;
    if timestamp >= auction_finish_time {
        return 0;
    }

    let mut current_rate_bump = data.initial_rate_bump as u64;
    let mut current_point_time = data.auction_start_time as u64;

    for point_and_time_delta in data.points_and_time_deltas.iter() {
        let next_rate_bump = point_and_time_delta.rate_bump as u64;
        let point_time_delta = point_and_time_delta.time_delta as u64;
        let next_point_time = current_point_time + point_time_delta;
        if timestamp <= next_point_time {
            return ((timestamp - current_point_time) * next_rate_bump
                + (next_point_time - timestamp) * current_rate_bump)
                / point_time_delta;
        }

        current_rate_bump = next_rate_bump;
        current_point_time = next_point_time;
    }
    (auction_finish_time - timestamp) * current_rate_bump
        / (auction_finish_time - current_point_time)
}
