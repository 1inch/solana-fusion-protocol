#[cfg(feature = "native")]
use solana_program::pubkey::Pubkey;

#[cfg(not(feature = "native"))]
use anchor_lang::prelude::*;

#[cfg(feature = "native")]
use borsh::{BorshDeserialize, BorshSerialize};

macro_rules! define_struct {
    (
        $(#[$struct_meta:meta])*
        $name:ident {
            $(
                $(#[$field_meta:meta])*
                $field:ident : $type:ty
            ),* $(,)?
        } $(, $($derive:ident),*)?
    ) => {
        #[cfg(feature = "native")]
        $(#[$struct_meta])*
        #[derive(BorshSerialize, BorshDeserialize $(, $($derive),*)?)]
        pub struct $name {
            $(
                $(#[$field_meta])*
                pub $field: $type
            ),*
        }

        #[cfg(not(feature = "native"))]
        $(#[$struct_meta])*
        #[derive(AnchorSerialize, AnchorDeserialize $(, $($derive),*)?)]
        pub struct $name {
            $(
                $(#[$field_meta])*
                pub $field: $type
            ),*
        }
    };
}

define_struct!(
    /// Configuration for fees applied to the escrow
    FeeConfig {
        protocol_dst_ata: Option<Pubkey>,
        integrator_dst_ata: Option<Pubkey>,

        /// Protocol fee in basis points where `BASE_1E5` = 100%
        protocol_fee: u16, 

        /// Integrator fee in basis points where `BASE_1E5` = 100%
        integrator_fee: u16,

        /// Percentage of positive slippage taken by the protocol as an additional fee. Value in basis points where `BASE_1E2` = 100%
        surplus_percentage: u8,
    }, Clone
);

define_struct!(
    /// Configuration for fees applied to the escrow
    ReducedFeeConfig {
        /// Protocol fee in basis points where `BASE_1E5` = 100%
        protocol_fee: u16,

        /// Integrator fee in basis points where `BASE_1E5` = 100%
        integrator_fee: u16,

        /// Percentage of positive slippage taken by the protocol as an additional fee. Value in basis points where `BASE_1E2` = 100%
        surplus_percentage: u8,
    }, Clone
);

define_struct!(
    OrderConfig {
        id: u32,
        src_amount: u64,
        min_dst_amount: u64,
        estimated_dst_amount: u64,
        expiration_time: u32,
        native_dst_asset: bool,
        receiver: Pubkey,
        fee: FeeConfig,
        dutch_auction_data: DutchAuctionData,
        src_mint: Pubkey,
        dst_mint: Pubkey,
    }
);

define_struct!(
    ReducedOrderConfig {
        id: u32,
        src_amount: u64,
        min_dst_amount: u64,
        estimated_dst_amount: u64,
        expiration_time: u32,
        native_dst_asset: bool,
        fee: ReducedFeeConfig,
        dutch_auction_data: DutchAuctionData,
    }
);

define_struct!(
    PointAndTimeDelta {
        rate_bump: u16,
        time_delta: u16,
    }, Clone
);

define_struct!(
    DutchAuctionData {
        start_time: u32,
        duration: u32,
        initial_rate_bump: u16,
        points_and_time_deltas: Vec<PointAndTimeDelta>,
    }, Clone
);
