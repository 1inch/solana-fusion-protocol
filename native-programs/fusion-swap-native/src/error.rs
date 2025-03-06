use spl_program_error::num_traits;
use spl_program_error::spl_program_error;
#[spl_program_error]
pub enum FusionError {
    #[error("A token mint constraint was violated")]
    ConstraintTokenMint = 2014,
    #[error("A signer constraint was violated")]
    ConstraintSigner = 2002,
    #[error("The given account is not mutable")]
    AccountNotWritable = 3006,
    #[error("An owner constraint was violated")]
    ConstraintOwner = 2004,
    #[error("A token owner constraint was violated")]
    ConstraintTokenOwner = 2015,
    #[error("A mint token program constraint was violated")]
    ConstraintMintTokenProgram = 2022,
    #[error("A seeds constraint was violated")]
    ConstraintSeeds = 2006,
    #[error("An address constraint was violated")]
    ConstraintAddress = 2012,
    #[error("The given account is not the associated token account")]
    AccountNotAssociatedTokenAccount = 3014,
    #[error("Inconsistent native dst trait")]
    InconsistentNativeDstTrait = 6000,
    #[error("Invalid amount")]
    InvalidAmount = 6001,
    #[error("Missing maker dst ata")]
    MissingMakerDstAta = 6002,
    #[error("Not enough tokens in escrow")]
    NotEnoughTokensInEscrow = 6003,
    #[error("Order expired")]
    OrderExpired = 6004,
    #[error("Invalid estimated taking amount")]
    InvalidEstimatedTakingAmount = 6005,
    #[error("Protocol surplus fee too high")]
    InvalidProtocolSurplusFee = 6006,
    #[error("Inconsistent protocol fee config")]
    InconsistentProtocolFeeConfig = 6007,
    #[error("Inconsistent integrator fee config")]
    InconsistentIntegratorFeeConfig = 6008,
}
