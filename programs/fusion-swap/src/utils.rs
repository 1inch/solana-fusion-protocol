use anchor_lang::prelude::*;
use anchor_lang::solana_program::system_program;

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
                    "escrow".as_bytes(),
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
            "escrow".as_bytes(),
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

// Function to get amount of `y_mint` tokens that the taker should pay to the maker using the default formula
pub fn get_y_amount(escrow_x_amount: u64, escrow_y_amount: u64, swap_amount: u64) -> u64 {
    (swap_amount * escrow_y_amount).div_ceil(escrow_x_amount)
}
