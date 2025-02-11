use anchor_lang::error_code;

#[error_code]
pub enum WhitelistError {
    #[msg("Unauthorized owner")]
    UnauthorizedOwner,
}
