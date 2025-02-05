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
    #[msg("Unexpected getter program")]
    UnexpectedGetterProgram,
    #[msg("Predicate is not satisfied")]
    PredicateNotSatisfied,
    #[msg("Unexpected predicate program")]
    UnexpectedPredicateProgram,
    #[msg("Invalid extension")]
    InvalidExtension,
    #[msg("Order id already used")]
    OrderIdAlreadyUsed,
    #[msg("Y amount exceeded")]
    YAmountExceeded,
    #[msg("Seller receiver mismatch")]
    SellerReceiverMismatch,
}
