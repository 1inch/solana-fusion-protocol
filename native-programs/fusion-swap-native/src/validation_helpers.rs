use solana_program::account_info::AccountInfo;
use solana_program::entrypoint::ProgramResult;
use solana_program::program::invoke;
use solana_program::program_error::ProgramError;
use solana_program::program_pack::Pack;
use solana_program::pubkey::Pubkey;
use spl_associated_token_account::get_associated_token_address_with_program_id;
use spl_associated_token_account::instruction::create_associated_token_account;
use spl_token::state::Account;

use crate::error::EscrowError;

pub fn assert_ownership(account_info: &AccountInfo) -> ProgramResult {
    if *account_info.owner != crate::ID {
        return Result::Err(EscrowError::ConstraintOwner.into());
    }
    Ok(())
}

pub fn assert_signer(account_info: &AccountInfo) -> ProgramResult {
    if !account_info.is_signer {
        return Result::Err(EscrowError::ConstraintSigner.into());
    }
    Ok(())
}

pub fn assert_mint(account_info: &AccountInfo) -> ProgramResult {
    if *account_info.owner != spl_token::ID && *account_info.owner != spl_token_2022::ID {
        // @TODO sras, this is really insufficient to properly validate that an account is
        // a mint. Add further checks.
        return Result::Err(EscrowError::ConstraintTokenMint.into());
    }
    Ok(())
}

pub fn assert_mut(account_info: &AccountInfo) -> ProgramResult {
    if !account_info.is_writable {
        return Result::Err(EscrowError::AccountNotMutable.into());
    }
    Ok(())
}

pub fn assert_token_account(
    account_info: &AccountInfo,
    opt_mint: Option<&Pubkey>,
    opt_authority: Option<&Pubkey>,
    opt_token_program: Option<&Pubkey>,
) -> ProgramResult {
    // decode account data
    let data: &[u8] = &mut account_info.data.borrow();
    let acc_data = Account::unpack(data).unwrap();

    // check mint
    if let Some(mint) = opt_mint {
        if acc_data.mint != *mint {
            return Result::Err(EscrowError::ConstraintTokenMint.into());
        }
    };
    // check token account owner
    if let Some(exp_authority) = opt_authority {
        if acc_data.owner != *exp_authority {
            return Result::Err(EscrowError::ConstraintTokenOwner.into());
        }
    };
    // check token program of the account by checking
    // the solana account owner
    if let Some(token_program) = opt_token_program {
        if *account_info.owner != *token_program {
            return Result::Err(EscrowError::ConstraintMintTokenProgram.into());
        }
    };
    Ok(())
}

pub fn assert_pda(
    account_info: &AccountInfo,
    seeds: &[&[u8]],
    program: &Pubkey,
    ret_bump: Option<&mut u8>,
) -> ProgramResult {
    if let Some((pda, bump)) = Pubkey::try_find_program_address(seeds, program) {
        if *account_info.key != pda {
            return Result::Err(EscrowError::ConstraintSeeds.into());
        }

        // After successful validation, save the bump to the optionally provided reference.
        if let Some(bump_ref) = ret_bump {
            *bump_ref = bump;
        }
    } else {
        return Result::Err(EscrowError::ConstraintSeeds.into());
    }
    Ok(())
}

pub fn assert_key(account_info: &AccountInfo, exp_pubkey: &Pubkey) -> ProgramResult {
    if *account_info.key != *exp_pubkey {
        return Result::Err(EscrowError::ConstraintAddress.into());
    }
    Ok(())
}

pub fn init_with_check_ata(
    account_info: &AccountInfo,
    payer: &Pubkey,
    accounts: &[AccountInfo],
    mint: &Pubkey,
    authority: &Pubkey,
    token_program: &Pubkey,
) -> ProgramResult {
    // ensure the account does not exist already.
    if account_info.data_is_empty()
        && account_info.lamports() == 0
        && *account_info.owner == solana_program::system_program::ID
    {
        // Validate the account address
        let ata = get_associated_token_address_with_program_id(authority, mint, token_program);
        if ata != *account_info.key {
            return Result::Err(EscrowError::AccountNotAssociatedTokenAccount.into());
        }
        // create the associated token account
        let create_ix = create_associated_token_account(payer, authority, mint, &spl_token::ID);
        invoke(&create_ix, accounts)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use solana_program::account_info::AccountInfo;
    use solana_program::entrypoint::ProgramResult;
    use solana_program::instruction::AccountMeta;
    use solana_program::instruction::Instruction;
    use solana_program::program_error::ProgramError;
    use solana_program::program_pack::Pack;
    use solana_program::pubkey::Pubkey;
    use solana_program_test::tokio;
    use solana_program_test::{
        processor, BanksClientError, BanksTransactionResultWithMetadata, ProgramTest,
        ProgramTestContext,
    };
    use solana_sdk::account::AccountSharedData;
    use solana_sdk::signature::Signer;
    use solana_sdk::signer::keypair::Keypair;
    use solana_sdk::system_instruction;
    use solana_sdk::transaction::Transaction;
    use solana_sdk::transaction::TransactionError;
    use spl_token::instruction as spl_instruction;
    use spl_token::state::{Account, Mint};

    pub trait Expectation {
        type ExpectationType;
        fn expect_success(self);
        fn expect_error(self, expectation: Self::ExpectationType);
    }

    impl Expectation for Result<BanksTransactionResultWithMetadata, BanksClientError> {
        type ExpectationType = &'static str;
        fn expect_success(self) {
            self.unwrap();
        }
        fn expect_error(self, expectation: &'static str) {
            if let Result::Ok(result_with_metadata) = self {
                let logs = result_with_metadata.metadata.unwrap().log_messages;

                assert!(logs.iter().any(|x| x.contains(expectation)));
            }
        }
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

    pub async fn deploy_spl_token(ctx: &mut ProgramTestContext, decimals: u8) -> Keypair {
        // create mint account
        let mint_keypair = Keypair::new();
        let create_mint_acc_ix = system_instruction::create_account(
            &ctx.payer.pubkey(),
            &mint_keypair.pubkey(),
            1_000_000_000, // Some lamports to pay rent
            Mint::LEN as u64,
            &spl_token::ID,
        );

        // initialize mint account
        let initialize_mint_ix: Instruction = spl_instruction::initialize_mint(
            &spl_token::ID,
            &mint_keypair.pubkey(),
            &ctx.payer.pubkey(),
            Option::None,
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
        let key = Pubkey::new_unique();
        let asd = AccountSharedData::new(1_000_000, 10, &crate::ID);
        ctx.set_account(&key, &asd);
        call_contract(&mut ctx, &[AccountMeta::new(key, false)])
            .await
            .expect_success();
    }

    #[tokio::test]
    async fn test_ownership_validation_fail() {
        let mut ctx = context_with_validation!(|x| assert_ownership(x));
        let key = Pubkey::new_unique();
        let random_address = Pubkey::new_unique();
        let asd = AccountSharedData::new(1_000_000, 10, &random_address);
        ctx.set_account(&key, &asd);
        call_contract(&mut ctx, &[AccountMeta::new(key, false)])
            .await
            .expect_error((0, EscrowError::ConstraintOwner.into()));
    }

    #[tokio::test]
    async fn test_mutability_validation() {
        let mut ctx = context_with_validation!(|x| assert_mut(x));
        let key = Pubkey::new_unique();
        let asd = AccountSharedData::new(1_000_000, 10, &crate::ID);
        ctx.set_account(&key, &asd);
        call_contract(&mut ctx, &[AccountMeta::new(key, false)])
            .await
            .expect_success();
    }

    #[tokio::test]
    async fn test_mutability_validation_fail() {
        let mut ctx = context_with_validation!(|x| assert_mut(x));
        let key = Pubkey::new_unique();
        let random_address = Pubkey::new_unique();
        let asd = AccountSharedData::new(1_000_000, 10, &random_address);
        ctx.set_account(&key, &asd);
        call_contract(&mut ctx, &[AccountMeta::new_readonly(key, false)])
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
        let random_address = Pubkey::new_unique();
        let asd = AccountSharedData::new(1_000_000, 10, &random_address);
        ctx.set_account(&random_address, &asd);
        call_contract(&mut ctx, &[AccountMeta::new(random_address, false)])
            .await
            .expect_error((0, EscrowError::ConstraintTokenMint.into()));
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
            let mut bump: u8 = 0;
            assert_pda(&accounts[0], &[b"escrow"], &crate::ID, Some(&mut bump))?;
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
        let mut ctx = context_with_validation!(|x| assert_pda(x, &[b"escrow"], &crate::ID, None,));

        let (pda, _) = Pubkey::find_program_address(&[b"bad"], &crate::ID);
        call_contract(&mut ctx, &[AccountMeta::new(pda, false)])
            .await
            .expect_error((0, EscrowError::ConstraintSeeds.into()));
    }

    #[tokio::test]
    async fn test_init_ata() {
        use spl_associated_token_account::get_associated_token_address_with_program_id;
        fn validation_test_contract(
            _: &Pubkey,
            accounts: &[AccountInfo],
            _: &[u8],
        ) -> ProgramResult {
            init_with_check_ata(
                &accounts[0],
                accounts[1].key,
                accounts,
                accounts[3].key,
                accounts[2].key,
                &spl_token::ID,
            )?;
            Ok(())
        }
        let program_test =
            ProgramTest::new("dummy", crate::ID, processor!(validation_test_contract));
        let mut ctx = program_test.start_with_context().await;
        let mut client = ctx.banks_client.clone();
        let mint_kp = deploy_spl_token(&mut ctx, 9).await;
        let alice = Pubkey::new_unique();
        let alice_ata =
            get_associated_token_address_with_program_id(&alice, &mint_kp.pubkey(), &spl_token::ID);
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
            init_with_check_ata(
                &accounts[0],
                accounts[1].key,
                accounts,
                accounts[3].key,
                accounts[2].key,
                &spl_token::ID,
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
