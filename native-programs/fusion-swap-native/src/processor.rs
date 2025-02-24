use solana_program::{account_info::AccountInfo, entrypoint::ProgramResult, msg, pubkey::Pubkey};

use spl_discriminator::{ArrayDiscriminator, SplDiscriminate};

pub struct Create;
pub struct Fill;
pub struct Cancel;

impl SplDiscriminate for Create {
    const SPL_DISCRIMINATOR: ArrayDiscriminator =
        ArrayDiscriminator::new([24, 30, 200, 40, 5, 28, 7, 119]);
}

impl SplDiscriminate for Fill {
    const SPL_DISCRIMINATOR: ArrayDiscriminator =
        ArrayDiscriminator::new([168, 96, 183, 163, 92, 10, 40, 160]);
}

impl SplDiscriminate for Cancel {
    const SPL_DISCRIMINATOR: ArrayDiscriminator =
        ArrayDiscriminator::new([232, 219, 223, 41, 219, 236, 220, 190]);
}

pub fn process(_program_id: &Pubkey, _accounts: &[AccountInfo], input: &[u8]) -> ProgramResult {
    let (discriminator, _) = input.split_at(ArrayDiscriminator::LENGTH);

    match discriminator {
        Create::SPL_DISCRIMINATOR_SLICE => process_create(),
        Fill::SPL_DISCRIMINATOR_SLICE => process_cancel(),
        Cancel::SPL_DISCRIMINATOR_SLICE => process_fill(),
        e => panic!("Unexpected discriminator in instruction! {:?}", e),
    }
}

fn process_create() -> ProgramResult {
    msg!("CREATE called!");
    Ok(())
}

fn process_cancel() -> ProgramResult {
    msg!("CANCEL called!");
    Ok(())
}

fn process_fill() -> ProgramResult {
    msg!("FILL called!");
    Ok(())
}
