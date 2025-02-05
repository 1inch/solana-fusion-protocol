use anchor_lang::prelude::*;
use anchor_lang::solana_program::{
    instruction::Instruction,
    keccak,
    program::{get_return_data, invoke},
    system_program,
};
use borsh::BorshSerialize;

// Flag that defines if the order can be filled partially
pub fn allow_partial_fills(traits: u8) -> bool {
    traits & 0b00000001 != 0
}

// Flag that defines if the order can be filled multiple times
pub fn allow_multiple_fills(traits: u8) -> bool {
    traits & 0b00000010 != 0
}

// Function to close the escrow account
pub fn close<'info>(
    token_program: AccountInfo<'info>,
    escrow: AccountInfo<'info>,
    escrowed_x_tokens: AccountInfo<'info>,
    escrowed_x_tokens_amount: u64,
    maker_x_token: AccountInfo<'info>,
    maker: AccountInfo<'info>,
    sol_receiver: AccountInfo<'info>,
    order_id: u32,
    escrow_bump: u8,
) -> Result<()> {
    // return maker's x_token back to account
    if escrowed_x_tokens_amount > 0 {
        anchor_spl::token::transfer(
            CpiContext::new_with_signer(
                token_program.clone(),
                anchor_spl::token::Transfer {
                    from: escrowed_x_tokens.to_account_info(),
                    to: maker_x_token,
                    authority: escrow.clone(),
                },
                &[&[
                    "escrow6".as_bytes(),
                    maker.key().as_ref(),
                    order_id.to_be_bytes().as_ref(),
                    &[escrow_bump],
                ]],
            ),
            escrowed_x_tokens_amount,
        )?;
    }

    // Close escrowed_x_tokens account
    anchor_spl::token::close_account(CpiContext::new_with_signer(
        token_program.clone(),
        anchor_spl::token::CloseAccount {
            account: escrowed_x_tokens.to_account_info(),
            destination: sol_receiver.to_account_info(),
            authority: escrow.clone(),
        },
        &[&[
            "escrow6".as_bytes(),
            maker.key().as_ref(),
            order_id.to_be_bytes().as_ref(),
            &[escrow_bump],
        ]],
    ))?;

    // Close escrow account
    close_account(escrow, sol_receiver)?;

    Ok(())
}

pub fn close_account<'info>(
    info: AccountInfo<'info>,
    sol_destination: AccountInfo<'info>,
) -> Result<()> {
    // Transfer tokens from the account to the sol_destination.
    let dest_starting_lamports = sol_destination.lamports();
    **sol_destination.lamports.borrow_mut() =
        dest_starting_lamports.checked_add(info.lamports()).unwrap();
    **info.lamports.borrow_mut() = 0;

    info.assign(&system_program::ID);
    info.realloc(0, false).map_err(Into::into)
}

pub fn calculate_extension_hash(
    opt_get_taking_amount_extra_data: &Option<Vec<u8>>,
    opt_predicate_extra_data: &Option<Vec<u8>>,
) -> Option<[u8; 16]> {
    if opt_get_taking_amount_extra_data.is_none() && opt_predicate_extra_data.is_none() {
        return None;
    }

    // Concatenate the extra data components
    let mut extra_data = vec![];
    if let Some(getter_extra_data) = opt_get_taking_amount_extra_data {
        extra_data.extend(getter_extra_data);
    }
    if let Some(predicate_extra_data) = opt_predicate_extra_data {
        extra_data.extend(predicate_extra_data);
    }

    let extra_data_hash = keccak::hash(&extra_data).0;
    Some(extra_data_hash[0..16].try_into().unwrap())
}

pub fn get_instruction_data<T: BorshSerialize>(instruction_name: &str, args: T) -> Vec<u8> {
    let mut ix_data = Vec::new();

    // Construct instruction data from the instruction discriminator and args
    let ix_discriminator = anchor_sighash(instruction_name);
    ix_data.extend_from_slice(&ix_discriminator);
    args.serialize(&mut ix_data).unwrap();

    ix_data
}

// TODO write a macro to calculate it in compile time
// Function to calculate the instruction discriminator
pub fn anchor_sighash(name: &str) -> [u8; 8] {
    let namespace = "global";
    let preimage = format!("{}:{}", namespace, name);
    let mut sighash = [0u8; 8];
    sighash.copy_from_slice(
        &anchor_lang::solana_program::hash::hash(preimage.as_bytes()).to_bytes()[..8],
    );
    sighash
}

#[error_code]
pub enum CallProgramError {
    #[msg("Program failed to execute")]
    ProgramFailedToExecute,
}

pub fn call_program<T: BorshSerialize>(
    program_id: Pubkey,
    instruction_name: &str,
    args: T,
) -> Result<Vec<u8>> {
    let call_program_ix = Instruction {
        program_id,
        accounts: vec![],
        data: get_instruction_data(instruction_name, args),
    };

    invoke(&call_program_ix, &[])?;

    let (pubkey, return_data) =
        get_return_data().ok_or(CallProgramError::ProgramFailedToExecute)?;
    if pubkey != program_id {
        return err!(CallProgramError::ProgramFailedToExecute);
    }
    Ok(return_data)
}

// Function to get amount of `y_mint` tokens that the taker should pay to the maker using the default formula
pub fn get_y_amount(escrow_x_amount: u64, escrow_y_amount: u64, swap_amount: u64) -> u64 {
    (swap_amount * escrow_y_amount).div_ceil(escrow_x_amount)
}

// Function to get amount of `x_mint` tokens that the taker will receive from the escrow using the default formula
pub fn get_x_amount(escrow_x_amount: u64, escrow_y_amount: u64, swap_amount: u64) -> u64 {
    swap_amount * escrow_x_amount / escrow_y_amount
}

// Function to get the amount of `y_mint` tokens that the taker should pay to the maker, supports getter program call
pub fn get_y_amount_with_getter(
    escrow_x_amount: u64,
    escrow_y_amount: u64,
    swap_amount: u64,
    provided_getter: Option<Pubkey>,
    data: &Option<Vec<u8>>,
) -> u64 {
    if let Some(getter_program) = provided_getter {
        let result = call_program(
            getter_program.key(),
            "calculate_taker_amount",
            GetAmountArgs {
                swap_amount,
                maker_amount: escrow_x_amount,
                taker_amount: escrow_y_amount,
                data: data.clone(),
            },
        )
        .unwrap();
        u64::try_from_slice(&result).unwrap()
    } else {
        get_y_amount(escrow_x_amount, escrow_y_amount, swap_amount)
    }
}

// Function to get the amount of `x_mint` tokens that the taker will receive from the escrow, supports getter program call
pub fn get_x_amount_with_getter(
    escrow_x_amount: u64,
    escrow_y_amount: u64,
    swap_amount: u64,
    provided_getter: Option<Pubkey>,
    data: &Option<Vec<u8>>,
) -> u64 {
    if let Some(getter_program) = provided_getter {
        let result = call_program(
            getter_program.key(),
            "calculate_maker_amount",
            GetAmountArgs {
                swap_amount,
                maker_amount: escrow_x_amount,
                taker_amount: escrow_y_amount,
                data: data.clone(),
            },
        )
        .unwrap();
        u64::try_from_slice(&result).unwrap()
    } else {
        get_x_amount(escrow_x_amount, escrow_y_amount, swap_amount)
    }
}

#[derive(BorshSerialize)]
pub struct GetAmountArgs {
    pub swap_amount: u64,
    pub maker_amount: u64,
    pub taker_amount: u64,
    pub data: Option<Vec<u8>>,
}
