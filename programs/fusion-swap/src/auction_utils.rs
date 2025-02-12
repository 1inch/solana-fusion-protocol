use anchor_lang::prelude::*;

#[derive(AnchorSerialize, AnchorDeserialize, Clone, InitSpace)]
pub struct PointsAndTimeDeltas {
    rate_bump: u32,
    point_time: u16, // delta between previous point and this point
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, InitSpace)]
pub struct DutchAuctionData {
    pub auction_start_time: u32,
    pub auction_finish_time: u32,
    pub initial_rate_bump: u32,
    #[max_len(5)]
    pub points_and_time_deltas: Vec<PointsAndTimeDeltas>,
}

pub fn calculate_rate_bump(timestamp: u32, data: DutchAuctionData) -> u64 {
    if timestamp <= data.auction_start_time {
        return data.initial_rate_bump as u64;
    }
    if timestamp >= data.auction_finish_time {
        return 0;
    }

    let mut current_rate_bump = data.initial_rate_bump as u64;
    let mut current_point_time = data.auction_start_time as u64;

    for point_and_time_delta in data.points_and_time_deltas.iter() {
        let next_rate_bump = point_and_time_delta.rate_bump as u64;
        let next_point_time = current_point_time + point_and_time_delta.point_time as u64;
        if timestamp as u64 <= next_point_time {
            return ((timestamp as u64 - current_point_time) * next_rate_bump
                + (next_point_time - timestamp as u64) * current_rate_bump)
                / (next_point_time - current_point_time);
        }

        current_rate_bump = next_rate_bump;
        current_point_time = next_point_time;
    }
    (data.auction_finish_time as u64 - timestamp as u64) * current_rate_bump
        / (data.auction_finish_time as u64 - current_point_time)
}
