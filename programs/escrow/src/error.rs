use anchor_lang::error_code;

#[error_code]
pub enum EscrowError {
    #[msg("Order expired")]
    OrderExpired,
    #[msg("Private order")]
    PrivateOrder,
    #[msg("Partial fill not allowed")]
    PartialFillNotAllowed,
    #[msg("Not enough tokens in escrow")]
    NotEnoughTokensInEscrow,
    #[msg("Seller receiver mismatch")]
    SellerReceiverMismatch,
}
