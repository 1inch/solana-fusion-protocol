use anchor_lang::prelude::*;

/// Max amount of points in the dutch auction data
pub const MAX_AUCTION_POINTS: usize = 5;

#[derive(AnchorSerialize, AnchorDeserialize, Clone, InitSpace)]
pub struct PointAndTimeDelta {
    rate_bump: u16,
    time_delta: u16,
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, InitSpace)]
pub struct DutchAuctionData {
    pub start_time: u32,
    pub duration: u32,
    pub initial_rate_bump: u16,
    #[max_len(MAX_AUCTION_POINTS)]
    pub points_and_time_deltas: Vec<PointAndTimeDelta>,
}

pub fn calculate_rate_bump(timestamp: u64, data: &DutchAuctionData) -> u64 {
    if timestamp <= data.start_time as u64 {
        return data.initial_rate_bump as u64;
    }
    let auction_finish_time = data.start_time as u64 + data.duration as u64;
    if timestamp >= auction_finish_time {
        return 0;
    }

    let mut current_rate_bump = data.initial_rate_bump as u64;
    let mut current_point_time = data.start_time as u64;

    for point_and_time_delta in data.points_and_time_deltas.iter() {
        let next_rate_bump = point_and_time_delta.rate_bump as u64;
        let point_time_delta = point_and_time_delta.time_delta as u64;
        let next_point_time = current_point_time + point_time_delta;

        if timestamp <= next_point_time {
            // Overflow is not possible because:
            // 1. current_point_time < timestamp <= next_point_time
            // 2. size(timestamp) + size(rate_bump) < 64
            // 3. point_time_delta != 0 as this would contradict point 1
            return ((timestamp - current_point_time) * next_rate_bump
                + (next_point_time - timestamp) * current_rate_bump)
                / point_time_delta;
        }

        current_rate_bump = next_rate_bump;
        current_point_time = next_point_time;
    }

    // Overflow is not possible because:
    // 1. timestamp < auction_finish_time
    // 2. size(timestamp) + size(rate_bump) < 64
    // 3. current_point_time < auction_finish_time as we know that current_point_time < timestamp
    (auction_finish_time - timestamp) * current_rate_bump
        / (auction_finish_time - current_point_time)
}
