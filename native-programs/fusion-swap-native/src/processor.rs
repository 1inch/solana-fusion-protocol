use solana_program::{
    account_info::{next_account_info, AccountInfo},
    entrypoint::ProgramResult,
    msg,
    program::invoke_signed,
    program_error::ProgramError,
    program_pack::Pack,
    pubkey::Pubkey,
    sysvar::{clock::Clock, Sysvar},
};

use spl_token::{
    instruction as token_instruction,
    state::{Account, Mint},
};

use borsh::BorshDeserialize;
use spl_discriminator::{ArrayDiscriminator, SplDiscriminate};

use crate::types::{build_order_from_reduced, order_hash, DutchAuctionData, ReducedOrderConfig};
use crate::{
    error::EscrowError,
    validation_helpers::{assert_pda, assert_signer, assert_token_program, assert_writable},
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
        Cancel::SPL_DISCRIMINATOR_SLICE => process_cancel(),
        Fill::SPL_DISCRIMINATOR_SLICE => process_fill(accounts, input),
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

fn process_fill(accounts: &[AccountInfo], input: &[u8]) -> ProgramResult {
    let account_info_iter = &mut accounts.iter();

    let taker = next_account_info(account_info_iter)?;
    let _resolver_access = next_account_info(account_info_iter)?;
    let maker = next_account_info(account_info_iter)?;
    let maker_receiver = next_account_info(account_info_iter)?;
    let src_mint = next_account_info(account_info_iter)?;
    let dst_mint = next_account_info(account_info_iter)?;
    let escrow = next_account_info(account_info_iter)?;
    let escrow_src_ata = next_account_info(account_info_iter)?;
    let maker_dst_ata = account_info_iter.next();
    let protocol_dst_ata = account_info_iter.next();
    let integrator_dst_ata = account_info_iter.next();
    let taker_src_ata = next_account_info(account_info_iter)?;
    let taker_dst_ata = next_account_info(account_info_iter)?;
    let src_token_program = next_account_info(account_info_iter)?;
    let dst_token_program = next_account_info(account_info_iter)?;
    let system_program = next_account_info(account_info_iter)?;
    let _associated_token_program = next_account_info(account_info_iter)?;

    // Deserialize input
    let (reduced_order_data, remaining_data) =
        input.split_at(std::mem::size_of::<ReducedOrderConfig>());
    let reduced_order = ReducedOrderConfig::try_from_slice(reduced_order_data)
        .map_err(|_| ProgramError::InvalidInstructionData)?;
    let amount = u64::from_le_bytes(remaining_data[..8].try_into().unwrap());

    // Checks
    assert_signer(taker)?;
    // TODO: add validation that account allowed to fill the order
    // assert_fillable(resolver_access)?;
    assert_writable(escrow_src_ata)?;
    assert_writable(taker_src_ata)?;
    assert_writable(taker_dst_ata)?;
    assert_token_program(src_token_program)?;
    assert_token_program(dst_token_program)?;

    // Check order is not expired
    let clock = Clock::get()?;
    if clock.unix_timestamp > reduced_order.expiration_time as i64 {
        return Err(EscrowError::OrderExpired.into());
    }

    // Deserialize SPL-token accounts
    let escrow_src_ata_data = Account::unpack(&escrow_src_ata.try_borrow_data()?)?;
    let src_mint_data = Mint::unpack(&src_mint.try_borrow_data()?)?;

    // Check that the escrow has enough tokens
    if amount > escrow_src_ata_data.amount {
        return Err(EscrowError::NotEnoughTokensInEscrow.into());
    }
    if amount == 0 {
        return Err(EscrowError::InvalidAmount.into());
    }

    // Repair full OrderConfig
    let order = build_order_from_reduced(
        &reduced_order,
        *src_mint.key,
        *dst_mint.key,
        *maker_receiver.key,
        protocol_dst_ata.map(|ata| *ata.key),
        integrator_dst_ata.map(|ata| *ata.key),
    );

    // Calculate order hash
    let order_hash_array = order_hash(&order)?;

    // Check escrow is PDA
    let bump = assert_pda(
        escrow,
        &[b"escrow", maker.key.as_ref(), &order_hash_array],
        &crate::ID,
    )?;

    // Transfer tokens from escrow to taker
    let transfer_instruction = token_instruction::transfer_checked(
        src_token_program.key,
        escrow_src_ata.key,
        src_mint.key,
        taker_src_ata.key,
        escrow.key,
        &[],
        amount,
        src_mint_data.decimals,
    )?;

    invoke_signed(
        &transfer_instruction,
        &[
            escrow_src_ata.clone(),
            src_mint.clone(),
            taker_src_ata.clone(),
            escrow.clone(),
            src_token_program.clone(),
        ],
        &[&[b"escrow", maker.key.as_ref(), &order_hash_array, &[bump]]],
    )?;

    // Calculate fees and transfer
    let dst_amount = get_dst_amount(
        order.src_amount,
        order.min_dst_amount,
        amount,
        Some(&order.dutch_auction_data),
    )?;

    let (protocol_fee_amount, integrator_fee_amount, maker_dst_amount) = get_fee_amounts(
        order.fee.integrator_fee,
        order.fee.protocol_fee,
        order.fee.surplus_percentage,
        dst_amount,
        get_dst_amount(order.src_amount, order.estimated_dst_amount, amount, None)?,
    )?;

    // Оплата протокольной комиссии
    if protocol_fee_amount > 0 {
        let protocol_dst_ata =
            protocol_dst_ata.ok_or(EscrowError::InconsistentProtocolFeeConfig)?;
        let protocol_transfer = token_instruction::transfer_checked(
            dst_token_program.key,
            taker_dst_ata.key,
            dst_mint.key,
            protocol_dst_ata.key,
            taker.key,
            &[],
            protocol_fee_amount,
            src_mint_data.decimals,
        )?;
        invoke_signed(
            &protocol_transfer,
            &[
                taker_dst_ata.clone(),
                dst_mint.clone(),
                protocol_dst_ata.clone(),
                taker.clone(),
                dst_token_program.clone(),
            ],
            &[],
        )?;
    }

    // Integrator fee
    if integrator_fee_amount > 0 {
        let integrator_dst_ata =
            integrator_dst_ata.ok_or(EscrowError::InconsistentIntegratorFeeConfig)?;
        let integrator_transfer = token_instruction::transfer_checked(
            dst_token_program.key,
            taker_dst_ata.key,
            dst_mint.key,
            integrator_dst_ata.key,
            taker.key,
            &[],
            integrator_fee_amount,
            src_mint_data.decimals,
        )?;
        invoke_signed(
            &integrator_transfer,
            &[
                taker_dst_ata.clone(),
                dst_mint.clone(),
                integrator_dst_ata.clone(),
                taker.clone(),
                dst_token_program.clone(),
            ],
            &[],
        )?;
    }

    // Transfer the rest to maker
    if order.native_dst_asset {
        let transfer_native = solana_program::system_instruction::transfer(
            taker.key,
            maker_receiver.key,
            maker_dst_amount,
        );
        invoke_signed(
            &transfer_native,
            &[
                taker.clone(),
                maker_receiver.clone(),
                system_program.clone(),
            ],
            &[],
        )?;
    } else {
        let maker_dst_ata = maker_dst_ata.ok_or(EscrowError::MissingMakerDstAta)?;
        assert_writable(maker_dst_ata)?;
        let maker_transfer = token_instruction::transfer_checked(
            dst_token_program.key,
            taker_dst_ata.key,
            dst_mint.key,
            maker_dst_ata.key,
            taker.key,
            &[],
            maker_dst_amount,
            src_mint_data.decimals,
        )?;
        invoke_signed(
            &maker_transfer,
            &[
                taker_dst_ata.clone(),
                dst_mint.clone(),
                maker_dst_ata.clone(),
                taker.clone(),
                dst_token_program.clone(),
            ],
            &[],
        )?;
    }

    // Close escrow if all tokens are spent
    if escrow_src_ata_data.amount == amount {
        let close_instruction = token_instruction::close_account(
            src_token_program.key,
            escrow_src_ata.key,
            maker.key,
            escrow.key,
            &[],
        )?;
        invoke_signed(
            &close_instruction,
            &[
                escrow_src_ata.clone(),
                maker.clone(),
                escrow.clone(),
                src_token_program.clone(),
            ],
            &[&[b"escrow", maker.key.as_ref(), &order_hash_array, &[bump]]],
        )?;
    }

    Ok(())
}

pub fn get_dst_amount(
    _: u64,
    _: u64,
    _: u64,
    _: Option<&DutchAuctionData>,
) -> Result<u64, ProgramError> {
    // TODO: Implement
    Ok(0)
}

pub fn get_fee_amounts(
    _: u16,
    _: u16,
    _: u8,
    _: u64,
    _: u64,
) -> Result<(u64, u64, u64), ProgramError> {
    // TODO: Implement
    Ok((0, 0, 0))
}
