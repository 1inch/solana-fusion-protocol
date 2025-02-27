use borsh::{to_vec, BorshDeserialize, BorshSerialize};
use solana_program::{hash::hash, program_error::ProgramError, pubkey::Pubkey};
// use Result::*;

// TODO move to common crate

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

pub fn build_order_from_reduced(
    order: &ReducedOrderConfig,
    src_mint: Pubkey,
    dst_mint: Pubkey,
    receiver: Pubkey,
    protocol_dst_ata: Option<Pubkey>,
    integrator_dst_ata: Option<Pubkey>,
) -> OrderConfig {
    OrderConfig {
        id: order.id,
        src_amount: order.src_amount,
        min_dst_amount: order.min_dst_amount,
        estimated_dst_amount: order.estimated_dst_amount,
        expiration_time: order.expiration_time,
        native_dst_asset: order.native_dst_asset,
        receiver,
        fee: FeeConfig {
            protocol_dst_ata,
            integrator_dst_ata,
            protocol_fee: order.fee.protocol_fee,
            integrator_fee: order.fee.integrator_fee,
            surplus_percentage: order.fee.surplus_percentage,
        },
        dutch_auction_data: order.dutch_auction_data.clone(),
        src_mint,
        dst_mint,
    }
}

pub fn order_hash(order: &OrderConfig) -> Result<[u8; 32], ProgramError> {
    Ok(hash(to_vec(order)?.as_ref()).to_bytes())
}
