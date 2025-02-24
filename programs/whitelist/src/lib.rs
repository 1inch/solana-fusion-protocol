use borsh::{BorshDeserialize, BorshSerialize};
use solana_program::{
    account_info::{next_account_info, AccountInfo},
    entrypoint,
    entrypoint::ProgramResult,
    msg,
    program::invoke_signed,
    program_error::ProgramError,
    pubkey::Pubkey,
    rent::Rent,
    system_instruction,
    sysvar::Sysvar,
};

pub mod error;
use error::WhitelistError;

entrypoint!(process_instruction);

pub const WHITELIST_STATE_SEED: &[u8] = b"whitelist_state";
pub const RESOLVER_ACCESS_SEED: &[u8] = b"resolver_access";

#[derive(BorshSerialize, BorshDeserialize, Debug)]
pub struct WhitelistState {
    pub discriminator: u64,
    pub owner: Pubkey,
}

#[derive(BorshSerialize, BorshDeserialize, Debug)]
pub struct ResolverAccess {
    pub discriminator: u64,
}

impl WhitelistState {
    pub const DISCRIMINATOR: u64 = 0x1234567890abcdef;
}

impl ResolverAccess {
    pub const DISCRIMINATOR: u64 = 0xfedcba0987654321;
}

#[derive(BorshSerialize, BorshDeserialize, Debug)]
pub enum WhitelistInstruction {
    Initialize,
    Register { user: Pubkey },
    Deregister { user: Pubkey },
    TransferOwnership { new_owner: Pubkey },
}

pub fn process_instruction(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    instruction_data: &[u8],
) -> ProgramResult {
    let instruction = WhitelistInstruction::try_from_slice(instruction_data)?;

    match instruction {
        WhitelistInstruction::Initialize => {
            msg!("Instruction: Initialize");
            process_initialize(program_id, accounts)
        }
        WhitelistInstruction::Register { user } => {
            msg!("Instruction: Register");
            process_register(program_id, accounts, user)
        }
        WhitelistInstruction::Deregister { user } => {
            msg!("Instruction: Deregister");
            process_deregister(program_id, accounts, user)
        }
        WhitelistInstruction::TransferOwnership { new_owner } => {
            msg!("Instruction: TransferOwnership");
            process_transfer_ownership(program_id, accounts, new_owner)
        }
    }
}

fn process_initialize(program_id: &Pubkey, accounts: &[AccountInfo]) -> ProgramResult {
    let account_info_iter = &mut accounts.iter();
    let owner_info = next_account_info(account_info_iter)?;
    let whitelist_state_info = next_account_info(account_info_iter)?;
    let system_program_info = next_account_info(account_info_iter)?;

    if !owner_info.is_signer {
        return Err(ProgramError::MissingRequiredSignature);
    }

    let (whitelist_state_pda, bump_seed) = Pubkey::find_program_address(
        &[WHITELIST_STATE_SEED],
        program_id,
    );

    if whitelist_state_pda != *whitelist_state_info.key {
        return Err(ProgramError::InvalidSeeds);
    }

    let rent = Rent::get()?;
    let space = 8 + 32; // 8 bytes for discriminator + 32 bytes for Pubkey
    let rent_lamports = rent.minimum_balance(space);

    invoke_signed(
        &system_instruction::create_account(
            owner_info.key,
            whitelist_state_info.key,
            rent_lamports,
            space as u64,
            program_id,
        ),
        &[
            owner_info.clone(),
            whitelist_state_info.clone(),
            system_program_info.clone(),
        ],
        &[&[WHITELIST_STATE_SEED, &[bump_seed]]],
    )?;

    let whitelist_state = WhitelistState {
        discriminator: WhitelistState::DISCRIMINATOR,
        owner: *owner_info.key,
    };

    whitelist_state.serialize(&mut *whitelist_state_info.data.borrow_mut())?;
    Ok(())
}

fn process_register(program_id: &Pubkey, accounts: &[AccountInfo], user: Pubkey) -> ProgramResult {
    let account_info_iter = &mut accounts.iter();
    let owner_info = next_account_info(account_info_iter)?;
    let whitelist_state_info = next_account_info(account_info_iter)?;
    let resolver_access_info = next_account_info(account_info_iter)?;
    let system_program_info = next_account_info(account_info_iter)?;

    if !owner_info.is_signer {
        return Err(ProgramError::MissingRequiredSignature);
    }

    let whitelist_state = WhitelistState::try_from_slice(&whitelist_state_info.data.borrow())?;
    if whitelist_state.discriminator != WhitelistState::DISCRIMINATOR {
        return Err(ProgramError::InvalidAccountData);
    }
    if whitelist_state.owner != *owner_info.key {
        return Err(WhitelistError::UnauthorizedOwner.into());
    }

    let (resolver_access_pda, bump_seed) = Pubkey::find_program_address(
        &[RESOLVER_ACCESS_SEED, user.as_ref()],
        program_id,
    );

    if resolver_access_pda != *resolver_access_info.key {
        return Err(ProgramError::InvalidSeeds);
    }

    let rent = Rent::get()?;
    let space = 8; // 8 bytes for discriminator
    let rent_lamports = rent.minimum_balance(space);

    invoke_signed(
        &system_instruction::create_account(
            owner_info.key,
            resolver_access_info.key,
            rent_lamports,
            space as u64,
            program_id,
        ),
        &[
            owner_info.clone(),
            resolver_access_info.clone(),
            system_program_info.clone(),
        ],
        &[&[RESOLVER_ACCESS_SEED, user.as_ref(), &[bump_seed]]],
    )?;

    let resolver_access = ResolverAccess {
        discriminator: ResolverAccess::DISCRIMINATOR,
    };
    resolver_access.serialize(&mut *resolver_access_info.data.borrow_mut())?;
    Ok(())
}

fn process_deregister(program_id: &Pubkey, accounts: &[AccountInfo], user: Pubkey) -> ProgramResult {
    let account_info_iter = &mut accounts.iter();
    let owner_info = next_account_info(account_info_iter)?;
    let whitelist_state_info = next_account_info(account_info_iter)?;
    let resolver_access_info = next_account_info(account_info_iter)?;

    if !owner_info.is_signer {
        return Err(ProgramError::MissingRequiredSignature);
    }

    let whitelist_state = WhitelistState::try_from_slice(&whitelist_state_info.data.borrow())?;
    if whitelist_state.discriminator != WhitelistState::DISCRIMINATOR {
        return Err(ProgramError::InvalidAccountData);
    }
    if whitelist_state.owner != *owner_info.key {
        return Err(WhitelistError::UnauthorizedOwner.into());
    }

    let (resolver_access_pda, _) = Pubkey::find_program_address(
        &[RESOLVER_ACCESS_SEED, user.as_ref()],
        program_id,
    );

    if resolver_access_pda != *resolver_access_info.key {
        return Err(ProgramError::InvalidSeeds);
    }

    let dest_starting_lamports = owner_info.lamports();
    **owner_info.lamports.borrow_mut() = dest_starting_lamports
        .checked_add(resolver_access_info.lamports())
        .ok_or(ProgramError::ArithmeticOverflow)?;
    **resolver_access_info.lamports.borrow_mut() = 0;

    let mut source_data = resolver_access_info.data.borrow_mut();
    source_data.fill(0);

    Ok(())
}

fn process_transfer_ownership(
    _program_id: &Pubkey,
    accounts: &[AccountInfo],
    new_owner: Pubkey,
) -> ProgramResult {
    let account_info_iter = &mut accounts.iter();
    let current_owner_info = next_account_info(account_info_iter)?;
    let whitelist_state_info = next_account_info(account_info_iter)?;

    if !current_owner_info.is_signer {
        return Err(ProgramError::MissingRequiredSignature);
    }

    let whitelist_state = WhitelistState::try_from_slice(&whitelist_state_info.data.borrow())?;
    if whitelist_state.discriminator != WhitelistState::DISCRIMINATOR {
        return Err(ProgramError::InvalidAccountData);
    }
    if whitelist_state.owner != *current_owner_info.key {
        return Err(WhitelistError::UnauthorizedOwner.into());
    }

    // Create new state with updated owner
    let new_whitelist_state = WhitelistState {
        discriminator: WhitelistState::DISCRIMINATOR,
        owner: new_owner,
    };

    // Serialize the new state
    new_whitelist_state.serialize(&mut *whitelist_state_info.data.borrow_mut())?;

    Ok(())
}
