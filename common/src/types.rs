#[cfg(not(feature = "native"))]
use anchor_lang::{prelude::*, AnchorDeserialize, AnchorSerialize};

#[cfg(feature = "native")]
use solana_program::pubkey::Pubkey;

#[cfg(feature = "native")]
use borsh::{BorshDeserialize, BorshSerialize};

#[cfg_attr(feature = "native", derive(BorshSerialize, BorshDeserialize, Clone))]
#[cfg_attr(
    not(feature = "native"),
    derive(AnchorSerialize, AnchorDeserialize, Clone)
)]
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
}

/// Configuration for fees applied to the escrow
#[cfg_attr(feature = "native", derive(BorshSerialize, BorshDeserialize, Clone))]
#[cfg_attr(
    not(feature = "native"),
    derive(AnchorSerialize, AnchorDeserialize, Clone)
)]
pub struct ReducedFeeConfig {
    /// Protocol fee in basis points where `BASE_1E5` = 100%
    pub protocol_fee: u16,

    /// Integrator fee in basis points where `BASE_1E5` = 100%
    pub integrator_fee: u16,

    /// Percentage of positive slippage taken by the protocol as an additional fee.
    /// Value in basis points where `BASE_1E2` = 100%
    pub surplus_percentage: u8,
}

#[cfg_attr(feature = "native", derive(BorshSerialize, BorshDeserialize))]
#[cfg_attr(not(feature = "native"), derive(AnchorSerialize, AnchorDeserialize))]
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
    pub src_mint: Pubkey,
    pub dst_mint: Pubkey,
}

#[cfg_attr(feature = "native", derive(BorshSerialize, BorshDeserialize))]
#[cfg_attr(not(feature = "native"), derive(AnchorSerialize, AnchorDeserialize))]
pub struct ReducedOrderConfig {
    pub id: u32,
    pub src_amount: u64,
    pub min_dst_amount: u64,
    pub estimated_dst_amount: u64,
    pub expiration_time: u32,
    pub native_dst_asset: bool,
    pub fee: ReducedFeeConfig,
    pub dutch_auction_data: DutchAuctionData,
}

#[cfg_attr(feature = "native", derive(BorshSerialize, BorshDeserialize, Clone))]
#[cfg_attr(
    not(feature = "native"),
    derive(AnchorSerialize, AnchorDeserialize, Clone)
)]
pub struct PointAndTimeDelta {
    pub rate_bump: u16,
    pub time_delta: u16,
}

#[cfg_attr(feature = "native", derive(BorshSerialize, BorshDeserialize, Clone))]
#[cfg_attr(
    not(feature = "native"),
    derive(AnchorSerialize, AnchorDeserialize, Clone)
)]
pub struct DutchAuctionData {
    pub start_time: u32,
    pub duration: u32,
    pub initial_rate_bump: u16,
    pub points_and_time_deltas: Vec<PointAndTimeDelta>,
}
