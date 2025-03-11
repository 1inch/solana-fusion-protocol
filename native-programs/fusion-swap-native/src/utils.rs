use crate::types::*;
use borsh::to_vec;
use solana_program::{hash::hash, program_error::ProgramError, pubkey::Pubkey};

pub fn build_order_from_reduced(
    order: &ReducedOrderConfig,
    src_mint: Pubkey,
    dst_mint: Pubkey,
    receiver: Pubkey,
    protocol_dst_acc: Option<Pubkey>,
    integrator_dst_acc: Option<Pubkey>,
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
            protocol_dst_acc,
            integrator_dst_acc,
            protocol_fee: order.fee.protocol_fee,
            integrator_fee: order.fee.integrator_fee,
            surplus_percentage: order.fee.surplus_percentage,
            min_cancellation_premium: order.fee.min_cancellation_premium,
            max_cancellation_multiplier: order.fee.max_cancellation_multiplier,
        },
        dutch_auction_data: order.dutch_auction_data.clone(),
        cancellation_auction_duration: order.cancellation_auction_duration,
        src_mint,
        dst_mint,
    }
}

pub fn order_hash(order: &OrderConfig) -> Result<[u8; 32], ProgramError> {
    Ok(hash(to_vec(order)?.as_ref()).to_bytes())
}
