use anchor_lang::error_code;

#[error_code]
pub enum EscrowError {
    #[msg("Inconsistent native dst trait")]
    InconsistentNativeDstTrait,
    #[msg("Invalid amount")]
    InvalidAmount,
    #[msg("Mising makerDstAta")]
    MissingMakerDstAta,
    #[msg("Not enough tokens in escrow")]
    NotEnoughTokensInEscrow,
    #[msg("Order expired")]
    OrderExpired,
    #[msg("Partial fill not allowed")]
    PartialFillNotAllowed,
    #[msg("Private order")]
    PrivateOrder,
    #[msg("Seller receiver mismatch")]
    SellerReceiverMismatch,
}
