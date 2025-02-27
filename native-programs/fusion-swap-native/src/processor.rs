use solana_program::{
    account_info::{next_account_info, AccountInfo},
    entrypoint::ProgramResult,
    msg,
    program_error::ProgramError,
    pubkey::Pubkey,
};

use solana_program::program_pack::Pack;
use spl_discriminator::{ArrayDiscriminator, SplDiscriminate};
use spl_token::state::{Account, Mint};

use crate::validation_helpers::{
    assert_pda, assert_signer, assert_token_account, assert_token_program, assert_writable,
};

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

pub fn process(_program_id: &Pubkey, accounts: &[AccountInfo], input: &[u8]) -> ProgramResult {
    let (discriminator, _) = input.split_at(ArrayDiscriminator::LENGTH);

    match discriminator {
        Create::SPL_DISCRIMINATOR_SLICE => process_create(),
        Fill::SPL_DISCRIMINATOR_SLICE => process_fill(),
        Cancel::SPL_DISCRIMINATOR_SLICE => process_cancel(accounts, input),
        e => panic!("Unexpected discriminator in instruction! {:?}", e),
    }
}

fn process_create() -> ProgramResult {
    msg!("CREATE called!");
    Ok(())
}

fn process_cancel(accounts: &[AccountInfo], input: &[u8]) -> ProgramResult {
    let account_info_iter = &mut accounts.iter();

    let maker = next_account_info(account_info_iter)?;
    let src_mint = next_account_info(account_info_iter)?;
    let escrow = next_account_info(account_info_iter)?;
    let escrow_src_ata = next_account_info(account_info_iter)?;
    let maker_src_ata = next_account_info(account_info_iter)?;
    let src_token_program = next_account_info(account_info_iter)?;

    // Check accounts
    assert_signer(maker)?;
    assert_writable(escrow_src_ata)?;
    assert_writable(maker_src_ata)?;
    assert_writable(escrow)?;
    assert_token_program(src_token_program)?;

    // Decerealize order_hash
    let order_hash = input
        .get(ArrayDiscriminator::LENGTH..ArrayDiscriminator::LENGTH + 32)
        .ok_or(ProgramError::InvalidInstructionData)?;
    let order_hash_array: [u8; 32] = order_hash
        .try_into()
        .map_err(|_| ProgramError::InvalidInstructionData)?;

    // Check escrow PDA is correct
    let bump = assert_pda(
        escrow,
        &[b"escrow", maker.key.as_ref(), &order_hash_array],
        &crate::ID,
    )?;

    // Decerealize accounts
    let escrow_src_ata_data = Account::unpack(&escrow_src_ata.try_borrow_data()?)?;
    let src_mint_data = Mint::unpack(&src_mint.try_borrow_data()?)?;

    // Check escrow owns src_ata
    assert_token_account(
        escrow_src_ata,
        src_mint.key,
        Some(escrow.key),
        Some(&spl_token::ID),
    )?;
    assert_token_account(
        maker_src_ata,
        src_mint.key,
        Some(maker.key),
        Some(&spl_token::ID),
    )?;

    // Transfer tokens from escrow to maker
    let transfer_instruction = spl_token::instruction::transfer_checked(
        src_token_program.key,
        escrow_src_ata.key,
        src_mint.key,
        maker_src_ata.key,
        escrow.key, // escrow should be signed
        &[],
        escrow_src_ata_data.amount,
        src_mint_data.decimals,
    )?;

    solana_program::program::invoke_signed(
        &transfer_instruction,
        &[
            escrow_src_ata.clone(),
            src_mint.clone(),
            maker_src_ata.clone(),
            escrow.clone(),
            src_token_program.clone(),
        ],
        &[&[b"escrow", maker.key.as_ref(), &order_hash_array, &[bump]]],
    )?;

    // Close escrow account
    let close_instruction = spl_token::instruction::close_account(
        src_token_program.key,
        escrow_src_ata.key,
        maker.key,
        escrow.key,
        &[],
    )?;

    solana_program::program::invoke_signed(
        &close_instruction,
        &[
            escrow_src_ata.clone(),
            maker.clone(),
            escrow.clone(),
            src_token_program.clone(),
        ],
        &[&[b"escrow", maker.key.as_ref(), &order_hash_array, &[bump]]],
    )?;

    Ok(())
}

fn process_fill() -> ProgramResult {
    msg!("FILL called!");
    Ok(())
}
