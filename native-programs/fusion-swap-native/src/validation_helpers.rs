use solana_program::{
    account_info::AccountInfo, entrypoint::ProgramResult, program::invoke,
    program_error::ProgramError, program_pack::Pack, pubkey::Pubkey,
};
use spl_associated_token_account::instruction::create_associated_token_account;
use spl_token::state::Account;

use crate::error::EscrowError;
use Result::*;

pub fn assert_ownership(account_info: &AccountInfo) -> ProgramResult {
    if *account_info.owner != crate::ID {
        return Err(EscrowError::ConstraintOwner.into());
    }
    Ok(())
}

pub fn assert_signer(account_info: &AccountInfo) -> ProgramResult {
    if !account_info.is_signer {
        return Err(EscrowError::ConstraintSigner.into());
    }
    Ok(())
}

pub fn assert_mint(account_info: &AccountInfo) -> ProgramResult {
    if *account_info.owner != spl_token::ID && *account_info.owner != spl_token_2022::ID {
        // TODO This is really insufficient to properly validate that an account is
        // a mint. Add further checks.
        return Err(EscrowError::ConstraintTokenMint.into());
    }
    Ok(())
}

pub fn assert_writable(account_info: &AccountInfo) -> ProgramResult {
    if !account_info.is_writable {
        return Err(EscrowError::AccountNotMutable.into());
    }
    Ok(())
}

pub fn assert_token_account(
    account_info: &AccountInfo,
    opt_mint: Option<&Pubkey>,
    opt_authority: Option<&Pubkey>,
    token_program: &Pubkey,
) -> ProgramResult {
    // Decode account data
    let data: &[u8] = &mut account_info.data.borrow();
    let acc_data = Account::unpack(data)?;
    // TODO: Support spl-token-2022

    // Check mint
    if let Some(mint) = opt_mint {
        if acc_data.mint != *mint {
            return Err(EscrowError::ConstraintTokenMint.into());
        }
    };
    // Check token account owner
    if let Some(exp_authority) = opt_authority {
        // TODO Consider using associated token account check if needed (address was derived following ATA rules)
        if acc_data.owner != *exp_authority {
            return Err(EscrowError::ConstraintTokenOwner.into());
        }
    };
    // Check token program of the account by checking
    // the solana account owner
    if *account_info.owner != *token_program {
        return Err(EscrowError::ConstraintMintTokenProgram.into());
    }
    Ok(())
}

pub fn assert_pda(
    account_info: &AccountInfo,
    seeds: &[&[u8]],
    program: &Pubkey,
) -> Result<u8, ProgramError> {
    let (pda, bump) = Pubkey::try_find_program_address(seeds, program)
        .ok_or::<EscrowError>(EscrowError::ConstraintSeeds)?;
    if *account_info.key != pda {
        return Err(EscrowError::ConstraintSeeds.into());
    }
    Ok(bump)
}

pub fn assert_key(account_info: &AccountInfo, exp_pubkey: &Pubkey) -> ProgramResult {
    if *account_info.key != *exp_pubkey {
        return Err(EscrowError::ConstraintAddress.into());
    }
    Ok(())
}

pub fn assert_token_program(account_info: &AccountInfo) -> ProgramResult {
    if *account_info.key != spl_token::ID && *account_info.key != spl_token_2022::ID {
        return Err(EscrowError::ConstraintAddress.into());
    }
    Ok(())
}

pub fn init_ata_with_address_check(
    account_info: &AccountInfo,
    payer: &Pubkey,
    mint: &Pubkey,
    authority: &Pubkey,
    token_program: &Pubkey,
    accounts: &[AccountInfo], // Should contain all the accounts for account creation and better if it
                              // contains nothing else, because it is passed directly to the
                              // cpi call to create the account.
) -> ProgramResult {
    // Ensure the account does not exist already.
    if account_info.data_is_empty()
        && account_info.lamports() == 0
        && *account_info.owner == solana_program::system_program::ID
    {
        // Validate the account address
        let ata = spl_associated_token_account::get_associated_token_address_with_program_id(
            authority,
            mint,
            token_program,
        );

        // Validate ata
        if ata != *account_info.key {
            return Err(EscrowError::AccountNotAssociatedTokenAccount.into());
        }
        // Create the associated token account
        let create_ix = create_associated_token_account(payer, authority, mint, token_program);
        invoke(&create_ix, accounts)
    } else {
        Err(ProgramError::AccountAlreadyInitialized)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use solana_program::{
        account_info::AccountInfo, entrypoint::ProgramResult, instruction::AccountMeta,
        instruction::Instruction, program_error::ProgramError, program_pack::Pack, pubkey::Pubkey,
    };
    use solana_program_test::{
        processor, tokio, BanksClientError, BanksTransactionResultWithMetadata, ProgramTest,
        ProgramTestContext,
    };
    use solana_sdk::{
        account::AccountSharedData, signature::Signer, signer::keypair::Keypair,
        system_instruction, transaction::Transaction, transaction::TransactionError,
    };
    use spl_token::instruction as spl_instruction;
    use spl_token::state::{Account, Mint};

    pub trait Expectation {
        type ExpectationType;
        fn expect_success(self);
        fn expect_error(self, expectation: Self::ExpectationType);
    }

    impl Expectation for Result<(), BanksClientError> {
        type ExpectationType = (u8, ProgramError);
        fn expect_success(self) {
            self.unwrap()
        }
        fn expect_error(self, expectation: (u8, ProgramError)) {
            let (index, expected_program_error) = expectation;
            if let TransactionError::InstructionError(result_instr_idx, result_instr_error) = self
                .expect_err("Expected an error, but transaction succeeded")
                .unwrap()
            {
                let result_program_error: ProgramError = result_instr_error.try_into().unwrap();
                assert_eq!(
                    (index, expected_program_error),
                    (result_instr_idx, result_program_error)
                );
            } else {
                panic!("Unexpected error provided: {:?}", expected_program_error);
            }
        }
    }

    async fn call_contract(
        ctx: &mut ProgramTestContext,
        accounts: &[AccountMeta],
    ) -> Result<(), BanksClientError> {
        call_contract_with_data(ctx, accounts, vec![]).await
    }

    async fn call_contract_with_data(
        ctx: &mut ProgramTestContext,
        accounts: &[AccountMeta],
        data: Vec<u8>,
    ) -> Result<(), BanksClientError> {
        let ix = Instruction {
            program_id: crate::ID,
            accounts: Vec::from(accounts),
            data,
        };

        let tx = Transaction::new_signed_with_payer(
            &[ix],
            Some(&ctx.payer.pubkey()),
            &[&ctx.payer],
            ctx.last_blockhash,
        );

        let mut client = ctx.banks_client.clone();
        client.process_transaction(tx).await
    }

    pub async fn initialize_spl_associated_account(
        ctx: &mut ProgramTestContext,
        mint_pubkey: &Pubkey,
        account: &Pubkey,
    ) -> Pubkey {
        let ata = spl_associated_token_account::get_associated_token_address(account, mint_pubkey);
        let create_spl_acc_ix = create_associated_token_account(
            &ctx.payer.pubkey(),
            account,
            mint_pubkey,
            &spl_token::ID,
        );

        let signers: Vec<&Keypair> = vec![&ctx.payer];

        let client = &mut ctx.banks_client;
        client
            .process_transaction(Transaction::new_signed_with_payer(
                &[create_spl_acc_ix],
                Some(&ctx.payer.pubkey()),
                &signers,
                ctx.last_blockhash,
            ))
            .await
            .unwrap();
        ata
    }

    pub async fn deploy_spl_token(ctx: &mut ProgramTestContext, decimals: u8) -> Keypair {
        let mint_keypair = Keypair::new();
        // Create mint account
        let create_mint_acc_ix = system_instruction::create_account(
            &ctx.payer.pubkey(),
            &mint_keypair.pubkey(),
            1_000_000_000, // Some lamports to pay rent
            Mint::LEN as u64,
            &spl_token::ID,
        );

        // Initialize mint account
        let initialize_mint_ix: Instruction = spl_instruction::initialize_mint(
            &spl_token::ID,
            &mint_keypair.pubkey(),
            &ctx.payer.pubkey(),
            None, // Freeze authority pubkey
            decimals,
        )
        .unwrap();

        let signers: Vec<&Keypair> = vec![&ctx.payer, &mint_keypair];

        let client = &mut ctx.banks_client;
        client
            .process_transaction(Transaction::new_signed_with_payer(
                &[create_mint_acc_ix, initialize_mint_ix],
                Some(&ctx.payer.pubkey()),
                &signers,
                ctx.last_blockhash,
            ))
            .await
            .unwrap();
        mint_keypair
    }

    fn create_account_with_owner(ctx: &mut ProgramTestContext, owner: &Pubkey) -> Pubkey {
        let key = Pubkey::new_unique();
        let asd = AccountSharedData::new(1_000_000, 10, owner);
        ctx.set_account(&key, &asd);
        key
    }

    // Start a test context with a deployed test contract that embedds the provided
    // validation call on the first account.
    macro_rules! context_with_validation {
        ($x:expr) => {{
            fn validation_test_contract(
                _: &Pubkey,
                accounts: &[AccountInfo],
                _: &[u8],
            ) -> ProgramResult {
                $x(&accounts[0])?;
                Ok(())
            }
            let program_test =
                ProgramTest::new("dummy", crate::ID, processor!(validation_test_contract));
            let ctx = program_test.start_with_context().await;
            ctx
        }};
    }

    #[tokio::test]
    async fn test_ownership_validation() {
        let mut ctx = context_with_validation!(|x| assert_ownership(x));
        let key = create_account_with_owner(&mut ctx, &crate::ID);
        call_contract(&mut ctx, &[AccountMeta::new(key, false)])
            .await
            .expect_success();
    }

    #[tokio::test]
    async fn test_ownership_validation_fail() {
        let mut ctx = context_with_validation!(|x| assert_ownership(x));
        let random_address = Pubkey::new_unique();
        let key = create_account_with_owner(&mut ctx, &random_address);
        call_contract(&mut ctx, &[AccountMeta::new(key, false)])
            .await
            .expect_error((0, EscrowError::ConstraintOwner.into()));
    }

    #[tokio::test]
    async fn test_key_validation() {
        let mut ctx = context_with_validation!(|x| assert_key(x, &crate::ID));
        call_contract(&mut ctx, &[AccountMeta::new(crate::ID, false)])
            .await
            .expect_success();
    }

    #[tokio::test]
    async fn test_key_validation_fail() {
        let mut ctx = context_with_validation!(|x| assert_key(x, &crate::ID));
        call_contract(&mut ctx, &[AccountMeta::new(Pubkey::new_unique(), false)])
            .await
            .expect_error((0, EscrowError::ConstraintAddress.into()));
    }

    #[tokio::test]
    async fn test_token_interface_validation() {
        let mut ctx = context_with_validation!(|x| assert_token_program(x));
        call_contract(&mut ctx, &[AccountMeta::new(spl_token::ID, false)])
            .await
            .expect_success();
    }

    #[tokio::test]
    async fn test_token_interface_validation_2022() {
        let mut ctx = context_with_validation!(|x| assert_token_program(x));
        call_contract(&mut ctx, &[AccountMeta::new(spl_token_2022::ID, false)])
            .await
            .expect_success();
    }

    #[tokio::test]
    async fn test_token_interface_validation_fail() {
        let mut ctx = context_with_validation!(|x| assert_token_program(x));
        call_contract(&mut ctx, &[AccountMeta::new(Pubkey::new_unique(), false)])
            .await
            .expect_error((0, EscrowError::ConstraintAddress.into()));
    }

    #[tokio::test]
    async fn test_writability_validation() {
        let mut ctx = context_with_validation!(|x| assert_writable(x));
        call_contract(&mut ctx, &[AccountMeta::new(Pubkey::new_unique(), false)])
            .await
            .expect_success();
    }

    #[tokio::test]
    async fn test_writability_validation_fail() {
        let mut ctx = context_with_validation!(|x| assert_writable(x));
        call_contract(
            &mut ctx,
            &[AccountMeta::new_readonly(Pubkey::new_unique(), false)],
        )
        .await
        .expect_error((0, EscrowError::AccountNotMutable.into()));
    }

    #[tokio::test]
    async fn test_mint_validation() {
        let mut ctx = context_with_validation!(|x| assert_mint(x));
        let mint_kp = deploy_spl_token(&mut ctx, 9).await;
        call_contract(&mut ctx, &[AccountMeta::new(mint_kp.pubkey(), false)])
            .await
            .expect_success();
    }

    #[tokio::test]
    async fn test_mint_validation_fail() {
        let mut ctx = context_with_validation!(|x| assert_mint(x));
        call_contract(&mut ctx, &[AccountMeta::new(Pubkey::new_unique(), false)])
            .await
            .expect_error((0, EscrowError::ConstraintTokenMint.into()));
    }

    #[tokio::test]
    async fn test_token_account_validation() {
        fn validation_test_contract(
            _: &Pubkey,
            accounts: &[AccountInfo],
            _: &[u8],
        ) -> ProgramResult {
            assert_token_account(
                &accounts[0],
                Some(accounts[1].key),
                Some(accounts[2].key),
                &spl_token::ID,
            )?;
            Ok(())
        }
        let program_test =
            ProgramTest::new("dummy", crate::ID, processor!(validation_test_contract));
        let mut ctx = program_test.start_with_context().await;

        let user_pk = Pubkey::new_unique();
        let mint_kp = deploy_spl_token(&mut ctx, 9).await;
        let ata = initialize_spl_associated_account(&mut ctx, &mint_kp.pubkey(), &user_pk).await;
        call_contract(
            &mut ctx,
            &[
                AccountMeta::new(ata, false),
                AccountMeta::new(mint_kp.pubkey(), false),
                AccountMeta::new(user_pk, false),
            ],
        )
        .await
        .expect_success();
    }

    #[tokio::test]
    async fn test_token_account_validation_fail() {
        fn validation_test_contract(
            _: &Pubkey,
            accounts: &[AccountInfo],
            _: &[u8],
        ) -> ProgramResult {
            assert_token_account(
                &accounts[0],
                Some(accounts[1].key),
                Some(accounts[2].key),
                &spl_token::ID,
            )?;
            Ok(())
        }
        let program_test =
            ProgramTest::new("dummy", crate::ID, processor!(validation_test_contract));
        let mut ctx = program_test.start_with_context().await;

        let user_pk = Pubkey::new_unique();
        let bad_user_pk = Pubkey::new_unique();
        let mint_kp = deploy_spl_token(&mut ctx, 9).await;
        let ata =
            initialize_spl_associated_account(&mut ctx, &mint_kp.pubkey(), &bad_user_pk).await;
        call_contract(
            &mut ctx,
            &[
                AccountMeta::new(ata, false),
                AccountMeta::new(mint_kp.pubkey(), false),
                AccountMeta::new(user_pk, false),
            ],
        )
        .await
        .expect_error((0, EscrowError::ConstraintTokenOwner.into()));
    }

    #[tokio::test]
    async fn test_pda_validation() {
        fn validation_test_contract(
            _: &Pubkey,
            accounts: &[AccountInfo],
            instruction_data: &[u8],
        ) -> ProgramResult {
            // We pass expected bump as instruction data.
            let expected_bump: u8 = instruction_data[0];
            let bump = assert_pda(&accounts[0], &[b"escrow"], &crate::ID)?;
            // If bump was not updated as expected, we throw an error as well.
            if bump != expected_bump {
                return Err(ProgramError::Custom(0));
            }
            Ok(())
        }
        let program_test =
            ProgramTest::new("dummy", crate::ID, processor!(validation_test_contract));
        let mut ctx = program_test.start_with_context().await;

        let (pda, computed_bump) = Pubkey::find_program_address(&[b"escrow"], &crate::ID);
        call_contract_with_data(
            &mut ctx,
            &[AccountMeta::new(pda, false)],
            vec![computed_bump],
        )
        .await
        .expect_success();
    }

    #[tokio::test]
    async fn test_pda_validation_fail() {
        let mut ctx = context_with_validation!(|x| assert_pda(x, &[b"escrow"], &crate::ID));

        let (pda, _) = Pubkey::find_program_address(&[b"bad"], &crate::ID);
        call_contract(&mut ctx, &[AccountMeta::new(pda, false)])
            .await
            .expect_error((0, EscrowError::ConstraintSeeds.into()));
    }

    #[tokio::test]
    async fn test_init_ata() {
        fn validation_test_contract(
            _: &Pubkey,
            accounts: &[AccountInfo],
            _: &[u8],
        ) -> ProgramResult {
            init_ata_with_address_check(
                &accounts[0],
                accounts[1].key,
                accounts[3].key,
                accounts[2].key,
                &spl_token::ID,
                accounts,
            )?;
            Ok(())
        }
        let program_test =
            ProgramTest::new("dummy", crate::ID, processor!(validation_test_contract));
        let mut ctx = program_test.start_with_context().await;
        let mut client = ctx.banks_client.clone();
        let mint_kp = deploy_spl_token(&mut ctx, 9).await;
        let alice = Pubkey::new_unique();
        let alice_ata = spl_associated_token_account::get_associated_token_address_with_program_id(
            &alice,
            &mint_kp.pubkey(),
            &spl_token::ID,
        );
        let payer = ctx.payer.pubkey();

        call_contract(
            &mut ctx,
            &[
                AccountMeta::new(alice_ata, false),
                AccountMeta::new(payer, true),
                AccountMeta::new(alice, false),
                AccountMeta::new(mint_kp.pubkey(), false),
                AccountMeta::new(solana_program::system_program::ID, false),
                AccountMeta::new(spl_associated_token_account::ID, false),
                AccountMeta::new(spl_token::ID, false),
            ],
        )
        .await
        .expect_success();

        // Assert ATA attributes.
        let ata_data: Account = client.get_packed_account_data(alice_ata).await.unwrap();
        assert_eq!(ata_data.owner, alice);
        assert_eq!(ata_data.mint, mint_kp.pubkey());
    }

    #[tokio::test]
    async fn test_init_ata_fail() {
        fn validation_test_contract(
            _: &Pubkey,
            accounts: &[AccountInfo],
            _: &[u8],
        ) -> ProgramResult {
            init_ata_with_address_check(
                &accounts[0],
                accounts[1].key,
                accounts[3].key,
                accounts[2].key,
                &spl_token::ID,
                accounts,
            )?;
            Ok(())
        }
        let program_test =
            ProgramTest::new("dummy", crate::ID, processor!(validation_test_contract));
        let mut ctx = program_test.start_with_context().await;
        let mint_kp = deploy_spl_token(&mut ctx, 9).await;
        let alice = Pubkey::new_unique();
        let alice_bad_ata = Pubkey::new_unique();
        let payer = ctx.payer.pubkey();

        call_contract(
            &mut ctx,
            &[
                AccountMeta::new(alice_bad_ata, false),
                AccountMeta::new(payer, true),
                AccountMeta::new(alice, false),
                AccountMeta::new(mint_kp.pubkey(), false),
                AccountMeta::new(solana_program::system_program::ID, false),
                AccountMeta::new(spl_associated_token_account::ID, false),
                AccountMeta::new(spl_token::ID, false),
            ],
        )
        .await
        .expect_error((0, EscrowError::AccountNotAssociatedTokenAccount.into()));
    }
}
