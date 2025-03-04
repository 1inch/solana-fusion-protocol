use anchor_lang::error_code;

#[error_code]
pub enum FusionError {
    #[msg("Inconsistent native dst trait")]
    InconsistentNativeDstTrait,
    #[msg("Invalid amount")]
    InvalidAmount,
    #[msg("Missing maker dst ata")]
    MissingMakerDstAta,
    #[msg("Not enough tokens in escrow")]
    NotEnoughTokensInEscrow,
    #[msg("Order expired")]
    OrderExpired,
    #[msg("Invalid estimated taking amount")]
    InvalidEstimatedTakingAmount,
    #[msg("Protocol surplus fee too high")]
    InvalidProtocolSurplusFee,
    #[msg("Inconsistent protocol fee config")]
    InconsistentProtocolFeeConfig,
    #[msg("Inconsistent integrator fee config")]
    InconsistentIntegratorFeeConfig,
    #[msg("Order not expired")]
    OrderNotExpired,
    #[msg("Invalid cancellation fee")]
    InvalidCancellationFee,
    #[msg("Cancel order by resolver is forbidden")]
    CancelOrderByResolverIsForbidden,
}
