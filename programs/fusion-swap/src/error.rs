use anchor_lang::error_code;

#[error_code]
pub enum EscrowError {
    #[msg("Inconsistent native dst trait")]
    InconsistentNativeDstTrait,
    #[msg("Invalid amount")]
    InvalidAmount,
    #[msg("Not enough tokens in escrow")]
    NotEnoughTokensInEscrow,
    #[msg("Order expired")]
    OrderExpired,
    #[msg("Partial fill not allowed")]
    PartialFillNotAllowed,
    #[msg("Seller receiver mismatch")]
    SellerReceiverMismatch,
    #[msg("Invalid estimated taking amount")]
    InvalidEstimatedTakingAmount,
    #[msg("Protocol surplus fee too high")]
    InvalidProtocolSurplusFee,
    #[msg("Inconsistent protocol fee config")]
    InconsistentProtocolFeeConfig,
    #[msg("Inconsistent integrator fee config")]
    InconsistentIntegratorFeeConfig,
    #[msg("Integer overflow")]
    IntegerOverflow,
}
