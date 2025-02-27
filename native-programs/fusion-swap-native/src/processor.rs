use borsh::BorshDeserialize;
use solana_program::{
    account_info::{next_account_info, AccountInfo},
    clock::Clock,
    entrypoint::ProgramResult,
    msg,
    program::invoke,
    pubkey::Pubkey,
    system_program,
    sysvar::Sysvar,
};

use spl_discriminator::{ArrayDiscriminator, SplDiscriminate};
use spl_token::native_mint;

use crate::{
    error::EscrowError,
    types::{build_order_from_reduced, order_hash, ReducedOrderConfig},
    validation_helpers::{
        assert_key, assert_mint, assert_pda, assert_signer, assert_token_account,
        assert_token_program, assert_writable, init_ata_with_address_check,
    },
};

// TODO remove after merging PR with it
macro_rules! require {
    ($x:expr, $e: expr) => {{
        if !($x) {
            return Err($e);
        };
    }};
}

// TODO move to common crate
pub const BASE_1E2: u64 = 100;
pub const BASE_1E5: u64 = 100_000;

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

pub fn process(program_id: &Pubkey, accounts: &[AccountInfo], input: &[u8]) -> ProgramResult {
    let (discriminator, input_rest) = input.split_at(ArrayDiscriminator::LENGTH);

    match discriminator {
        Create::SPL_DISCRIMINATOR_SLICE => process_create(program_id, accounts, input_rest),
        Fill::SPL_DISCRIMINATOR_SLICE => process_cancel(),
        Cancel::SPL_DISCRIMINATOR_SLICE => process_fill(),
        e => panic!("Unexpected discriminator in instruction! {:?}", e),
    }
}

fn process_create(program_id: &Pubkey, accounts: &[AccountInfo], input: &[u8]) -> ProgramResult {
    let account_info_iter = &mut accounts.iter();

    // These accounts are required to be continuous subslice of 'accounts'
    // to avoid cloning during CPI.
    //
    // accounts[0..=5] sub-slice will be used to initialize escrow src ata,
    // and accounts[3..=6] sub-slice will be used to transfer tokens from maker
    // src ata to escrow src ata
    let maker = next_account_info(account_info_iter)?;
    let src_mint = next_account_info(account_info_iter)?;
    let system_program = next_account_info(account_info_iter)?;
    let src_token_program = next_account_info(account_info_iter)?;
    let escrow = next_account_info(account_info_iter)?;
    let escrow_src_ata = next_account_info(account_info_iter)?;
    let maker_src_ata = next_account_info(account_info_iter)?;

    let dst_mint = next_account_info(account_info_iter)?;
    let maker_receiver = next_account_info(account_info_iter)?;
    let associated_token_program = next_account_info(account_info_iter)?;
    // TODO handle optionals
    let protocol_dst_ata = Some(next_account_info(account_info_iter)?);
    let integrator_dst_ata = Some(next_account_info(account_info_iter)?);

    let order = ReducedOrderConfig::try_from_slice(input)?;
    let order_full = build_order_from_reduced(
        &order,
        *src_mint.key,
        *dst_mint.key,
        *maker_receiver.key,
        protocol_dst_ata.map(|a| *a.key),
        integrator_dst_ata.map(|a| *a.key),
    );
    let order_hash = order_hash(&order_full)?;

    // Maker validations
    assert_signer(maker)?;
    assert_writable(maker)?;

    // Src mint validations
    assert_mint(src_mint)?;

    // Dst mint validations
    assert_mint(dst_mint)?;

    // Maker src ata validations
    assert_writable(maker_src_ata)?;
    assert_token_account(
        maker_src_ata,
        src_mint.key,
        Some(maker.key),
        Some(src_token_program.key),
    )?;

    // Escrow validations
    let _escrow_bump = assert_pda(
        escrow,
        &["escrow".as_bytes(), maker.key.as_ref(), &order_hash],
        program_id,
    )?;

    // Protocol dst ata validations
    if let Some(protocol_dst_ata) = protocol_dst_ata {
        assert_token_account(protocol_dst_ata, dst_mint.key, None, None)?;
    }

    // Integrator dst ata validations
    if let Some(integrator_dst_ata) = integrator_dst_ata {
        assert_token_account(integrator_dst_ata, dst_mint.key, None, None)?;
    }

    // Associated token program validations
    assert_key(associated_token_program, &spl_associated_token_account::ID)?;

    // Src token program validations
    assert_token_program(src_token_program)?;

    // System program validations
    assert_key(system_program, &system_program::ID)?;

    init_ata_with_address_check(
        escrow_src_ata,
        maker.key,
        src_mint.key,
        escrow.key,
        src_token_program.key,
        // [maker, src_mint, system_program, src_token_program, escrow, escrow_src_ata]
        &accounts[0..=5],
    )?;

    require!(
        order.src_amount != 0 && order.min_dst_amount != 0,
        EscrowError::InvalidAmount.into()
    );

    // We support only original spl_token::native_mint
    require!(
        *dst_mint.key == native_mint::ID || !order.native_dst_asset,
        EscrowError::InconsistentNativeDstTrait.into()
    );

    require!(
        Clock::get()?.unix_timestamp <= order.expiration_time as i64,
        EscrowError::OrderExpired.into()
    );

    require!(
        order.fee.surplus_percentage as u64 <= BASE_1E2,
        EscrowError::InvalidProtocolSurplusFee.into()
    );

    require!(
        order.estimated_dst_amount >= order.min_dst_amount,
        EscrowError::InvalidEstimatedTakingAmount.into()
    );

    // Iff protocol fee or surplus is positive, protocol_dst_ata must be set
    require!(
        (order.fee.protocol_fee > 0 || order.fee.surplus_percentage > 0)
            == protocol_dst_ata.is_some(),
        EscrowError::InconsistentProtocolFeeConfig.into()
    );

    // Iff integrator fee is positive, integrator_dst_ata must be set
    require!(
        (order.fee.integrator_fee > 0) == integrator_dst_ata.is_some(),
        EscrowError::InconsistentIntegratorFeeConfig.into()
    );

    // TODO transfer checked
    let transfer_ix = spl_token::instruction::transfer(
        src_token_program.key,
        maker_src_ata.key,
        escrow_src_ata.key,
        escrow.key,
        &[], // signer pubkeys
        order.src_amount,
    )?;

    invoke(
        &transfer_ix,
        // [src_token_program, escrow, escrow_src_ata, maker_src_ata]
        &accounts[3..=6],
    )
}

fn process_cancel() -> ProgramResult {
    msg!("CANCEL called!");
    Ok(())
}

fn process_fill() -> ProgramResult {
    msg!("FILL called!");
    Ok(())
}
