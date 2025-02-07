use anchor_lang::prelude::*;
use anchor_lang::solana_program::system_program;

// Function to close the escrow account
pub fn close<'info>(
    token_program: AccountInfo<'info>,
    escrow: AccountInfo<'info>,
    escrow_src_ata: AccountInfo<'info>,
    remaining_amount: u64,
    maker_src_ata: AccountInfo<'info>,
    maker: AccountInfo<'info>,
    order_id: u32,
    escrow_bump: u8,
) -> Result<()> {
    // return maker's src_token back to account
    if remaining_amount > 0 {
        anchor_spl::token::transfer(
            CpiContext::new_with_signer(
                token_program.clone(),
                anchor_spl::token::Transfer {
                    from: escrow_src_ata.to_account_info(),
                    to: maker_src_ata,
                    authority: escrow.clone(),
                },
                &[&[
                    "escrow".as_bytes(),
                    maker.key().as_ref(),
                    order_id.to_be_bytes().as_ref(),
                    &[escrow_bump],
                ]],
            ),
            remaining_amount,
        )?;
    }

    // Close escrow_src_ata account
    anchor_spl::token::close_account(CpiContext::new_with_signer(
        token_program.clone(),
        anchor_spl::token::CloseAccount {
            account: escrow_src_ata.to_account_info(),
            destination: maker.to_account_info(),
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
    close_account(escrow, maker)?;

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

// Function to get amount of `dst_mint` tokens that the taker should pay to the maker using the default formula
pub fn get_dst_amount(escrow_src_amount: u64, escrow_dst_amount: u64, swap_amount: u64) -> u64 {
    (swap_amount * escrow_dst_amount).div_ceil(escrow_src_amount)
}
