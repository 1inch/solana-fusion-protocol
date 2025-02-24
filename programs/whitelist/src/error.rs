use solana_program::program_error::ProgramError;
use thiserror::Error;

#[derive(Error, Debug, Copy, Clone)]
pub enum WhitelistError {
    #[error("Unauthorized owner")]
    UnauthorizedOwner,
}

impl From<WhitelistError> for ProgramError {
    fn from(e: WhitelistError) -> Self {
        ProgramError::Custom(e as u32)
    }
}
