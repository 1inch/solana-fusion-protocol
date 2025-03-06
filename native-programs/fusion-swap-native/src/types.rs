use borsh::{BorshDeserialize, BorshSerialize};
use solana_program::pubkey::Pubkey;

// TODO move to common crate
//
// Since it's imposible to just move the struct definition to the common crate,
// because Anchor program requires 'AnchorSerialize' and 'AnchorDeserialize'
// traits to be derived, but native program requires 'BorshSerialize' and
// 'BorshDeserialize' traits to be derived, we can do 2 options
//
// 1. Write a macro for struct definition and expand it in the program
// 2. Write conditional deriving in the common crate based on #[cfg()] macro
/// Configuration for fees applied to the escrow
#[derive(BorshSerialize, BorshDeserialize, Clone)]
pub struct FeeConfig {
    pub protocol_dst_ata: Option<Pubkey>,
    pub integrator_dst_ata: Option<Pubkey>,

    /// Protocol fee in basis points where `BASE_1E5` = 100%
    pub protocol_fee: u16,

    /// Integrator fee in basis points where `BASE_1E5` = 100%
    pub integrator_fee: u16,

    /// Percentage of positive slippage taken by the protocol as an additional fee.
    /// Value in basis points where `BASE_1E2` = 100%
    pub surplus_percentage: u8,

    /// Fee charged to the maker if the order is cancelled by resolver
    /// Value in absolute token amount
    pub min_cancellation_premium: u64,

    /// Maximum cancellation premium multiplier
    /// Value in basis points where `BASE_1E3` = 100%
    pub max_cancellation_multiplier: u16,
}

/// Configuration for fees applied to the escrow
#[derive(BorshSerialize, BorshDeserialize, Clone)]
pub struct ReducedFeeConfig {
    /// Protocol fee in basis points where `BASE_1E5` = 100%
    pub protocol_fee: u16,

    /// Integrator fee in basis points where `BASE_1E5` = 100%
    pub integrator_fee: u16,

    /// Percentage of positive slippage taken by the protocol as an additional fee.
    /// Value in basis points where `BASE_1E2` = 100%
    pub surplus_percentage: u8,

    /// Fee charged to the maker if the order is cancelled by resolver
    /// Value in absolute token amount
    pub min_cancellation_premium: u64,
    /// Maximum cancellation premium multiplier
    /// Value in basis points where `BASE_1E3` = 100%
    pub max_cancellation_multiplier: u16,
}

#[derive(BorshSerialize, BorshDeserialize)]
pub struct OrderConfig {
    pub id: u32,
    pub src_amount: u64,
    pub min_dst_amount: u64,
    pub estimated_dst_amount: u64,
    pub expiration_time: u32,
    pub native_dst_asset: bool,
    pub receiver: Pubkey,
    pub fee: FeeConfig,
    pub dutch_auction_data: DutchAuctionData,
    pub cancellation_auction_duration: u32,
    pub src_mint: Pubkey,
    pub dst_mint: Pubkey,
}

#[derive(BorshSerialize, BorshDeserialize)]
pub struct ReducedOrderConfig {
    pub id: u32,
    pub src_amount: u64,
    pub min_dst_amount: u64,
    pub estimated_dst_amount: u64,
    pub expiration_time: u32,
    pub native_dst_asset: bool,
    pub fee: ReducedFeeConfig,
    pub dutch_auction_data: DutchAuctionData,
    pub cancellation_auction_duration: u32,
}

#[derive(BorshSerialize, BorshDeserialize, Clone)]
pub struct PointAndTimeDelta {
    pub rate_bump: u16,
    pub time_delta: u16,
}

#[derive(BorshSerialize, BorshDeserialize, Clone)]
pub struct DutchAuctionData {
    pub start_time: u32,
    pub duration: u32,
    pub initial_rate_bump: u16,
    pub points_and_time_deltas: Vec<PointAndTimeDelta>,
}
