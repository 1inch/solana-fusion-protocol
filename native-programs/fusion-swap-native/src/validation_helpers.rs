use solana_program::account_info::AccountInfo;
use solana_program::entrypoint::ProgramResult;
use solana_program::msg;
use solana_program::program::invoke;
use solana_program::program_error::ProgramError;
use solana_program::program_pack::Pack;
use solana_program::pubkey::Pubkey;
use solana_program::system_instruction;
use solana_program::sysvar::rent::Rent;
use spl_associated_token_account::get_associated_token_address_with_program_id;
use spl_associated_token_account::instruction::create_associated_token_account;
use spl_token::state::Account;

pub enum Validation<'a, 'b> {
    // Check if the account owner is the program
    Ownership,
    // Check if the account is a signer
    Signer,
    // Check if the account is a token mint
    Mint,
    // Check if the account is mutable
    Mut,
    // Check if the account is a token account with provided attributes matching
    TokenAccount {
        opt_mint: Option<&'a Pubkey>,
        opt_authority: Option<&'a Pubkey>,
        opt_token_program: Option<&'a Pubkey>,
    },
    // Check if the account's pubkey is the one provided
    Key(Pubkey),
    // Validates the seeds for a PDA
    Pda {
        seeds: &'a [&'a [u8]],
        program: &'a Pubkey,
        ret_bump: Option<&'a mut u8>,
    },
    // Initialize the associated token account
    InitAta {
        payer: &'a Pubkey,
        accounts: &'a [AccountInfo<'b>],
        mint: &'a Pubkey,
        authority: &'a Pubkey,
        token_program: &'a Pubkey,
    },
}

pub fn validate<'a, 'b, 'c, 'd, 'e, 'f>(
    validation_specs: &'a mut [(&'b AccountInfo<'d>, &'c mut [Validation<'e, 'f>])],
) -> ProgramResult {
    for (account_info, validation_spec) in validation_specs {
        for validation in &mut **validation_spec {
            match validation {
                Validation::Ownership => {
                    if *account_info.owner != crate::ID {
                        return Result::Err(ProgramError::Custom(0));
                    }
                }
                Validation::Signer => {
                    if !account_info.is_signer {
                        return Result::Err(ProgramError::Custom(0));
                    }
                }
                Validation::Mint => {
                    if *account_info.owner != spl_token::ID
                        && *account_info.owner != spl_token_2022::ID
                    {
                        return Result::Err(ProgramError::Custom(0));
                    }
                }
                Validation::Mut => {
                    if !account_info.is_writable {
                        return Result::Err(ProgramError::Custom(0));
                    }
                }
                Validation::TokenAccount {
                    opt_mint,
                    opt_authority,
                    opt_token_program,
                } => {
                    // decode account data
                    let data: &[u8] = &mut account_info.data.borrow();
                    let acc_data = Account::unpack(data).unwrap();

                    // check mint
                    if let Some(mint) = *opt_mint {
                        if acc_data.mint != *mint {
                            return Result::Err(ProgramError::Custom(0));
                        }
                    };
                    // check token account owner
                    if let Some(exp_authority) = *opt_authority {
                        if acc_data.owner != *exp_authority {
                            return Result::Err(ProgramError::Custom(0));
                        }
                    };
                    // check token program of the account by checking
                    // the solana account owner
                    if let Some(token_program) = *opt_token_program {
                        if *account_info.owner != *token_program {
                            return Result::Err(ProgramError::Custom(0));
                        }
                    };
                }
                Validation::Pda {
                    seeds,
                    program,
                    ret_bump,
                } => {
                    if let Some((pda, bump)) = Pubkey::try_find_program_address(seeds, program) {
                        if *account_info.key != pda {
                            return Result::Err(ProgramError::Custom(0));
                        }

                        // After successful validation, save the bump to the optionally provided reference.
                        if let Some(bump_ref) = ret_bump {
                            **bump_ref = bump;
                        }
                    } else {
                        return Result::Err(ProgramError::Custom(0));
                    }
                }
                Validation::Key(exp_pubkey) => {
                    if *account_info.key != *exp_pubkey {
                        return Result::Err(ProgramError::Custom(0));
                    }
                }
                Validation::InitAta {
                    payer,
                    accounts,
                    mint,
                    authority,
                    token_program,
                } => {
                    // ensure the account does not exist already.
                    if account_info.data_is_empty()
                        && account_info.lamports() == 0
                        && *account_info.owner == solana_program::system_program::ID
                    {
                        // Validate the account address
                        let ata = get_associated_token_address_with_program_id(
                            authority,
                            mint,
                            token_program,
                        );
                        if ata != *account_info.key {
                            return Result::Err(ProgramError::Custom(0));
                        }
                        // create the associated token account
                        let create_ix = create_associated_token_account(
                            &payer,
                            authority,
                            mint,
                            &spl_token::ID,
                        );
                        invoke(&create_ix, *accounts);
                    }
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use solana_program::account_info::AccountInfo;
    use solana_program::entrypoint;
    use solana_program::entrypoint::ProgramResult;
    use solana_program::instruction::AccountMeta;
    use solana_program::instruction::Instruction;
    use solana_program::msg;
    use solana_program::program_error::ProgramError;
    use solana_program::program_pack::Pack;
    use solana_program::pubkey::Pubkey;
    use solana_program_test::tokio;
    use solana_program_test::{
        processor, BanksClient, BanksClientError, BanksTransactionResultWithMetadata, ProgramTest,
        ProgramTestContext,
    };
    use solana_sdk::account::AccountSharedData;
    use solana_sdk::signature::Signer;
    use solana_sdk::signer::keypair::Keypair;
    use solana_sdk::system_instruction;
    use solana_sdk::transaction::Transaction;
    use solana_sdk::transaction::TransactionError;
    use spl_token::state::{Account, Mint};
    use spl_token::{instruction as spl_instruction, ID as spl_program_id};

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

    macro_rules! context_with_validation {
        ($(($x:expr, $v: expr)),*) => {{
            fn validation_test_contract(
                program_id: &Pubkey,
                accounts: &[AccountInfo],
                instruction_data: &[u8],
            ) -> ProgramResult {
                $(super::validate(&mut [(&accounts[$x], &mut $v)])?;)*
                    Ok(())
            }
            let mut program_test =
                ProgramTest::new("dummy", crate::ID, processor!(validation_test_contract));
            let mut ctx = program_test.start_with_context().await;
            ctx
        }};
    }

    #[tokio::test]
    async fn test_ownership_validation() {
        let mut ctx = context_with_validation!((0, [super::Validation::Ownership,]));
        let key = Pubkey::new_unique();
        let mut asd = AccountSharedData::new(1_000_000, 10, &crate::ID);
        ctx.set_account(&key, &asd);
        call_contract(&mut ctx, &[AccountMeta::new(key, false)])
            .await
            .expect_success();
    }

    #[tokio::test]
    async fn test_ownership_validation_fail() {
        let mut ctx = context_with_validation!((0, [super::Validation::Ownership,]));
        let key = Pubkey::new_unique();
        let random_address = Pubkey::new_unique();
        let mut asd = AccountSharedData::new(1_000_000, 10, &random_address);
        ctx.set_account(&key, &asd);
        call_contract(&mut ctx, &[AccountMeta::new(key, false)])
            .await
            .expect_error((0, ProgramError::Custom(0)));
    }

    #[tokio::test]
    async fn test_mutability_validation() {
        let mut ctx = context_with_validation!((0, [super::Validation::Mut,]));
        let key = Pubkey::new_unique();
        let mut asd = AccountSharedData::new(1_000_000, 10, &crate::ID);
        ctx.set_account(&key, &asd);
        call_contract(&mut ctx, &[AccountMeta::new(key, false)])
            .await
            .expect_success();
    }

    #[tokio::test]
    async fn test_mutability_validation_fail() {
        let mut ctx = context_with_validation!((0, [super::Validation::Mut,]));
        let key = Pubkey::new_unique();
        let random_address = Pubkey::new_unique();
        let mut asd = AccountSharedData::new(1_000_000, 10, &random_address);
        ctx.set_account(&key, &asd);
        call_contract(&mut ctx, &[AccountMeta::new_readonly(key, false)])
            .await
            .expect_error((0, ProgramError::Custom(0)));
    }

    #[tokio::test]
    async fn test_mint_validation() {
        let mut ctx = context_with_validation!((0, [super::Validation::Mint,]));
        let mint_kp = deploy_spl_token(&mut ctx, 9).await;
        call_contract(&mut ctx, &[AccountMeta::new(mint_kp.pubkey(), false)])
            .await
            .expect_success();
    }

    #[tokio::test]
    async fn test_mint_validation_fail() {
        let mut ctx = context_with_validation!((0, [super::Validation::Mint,]));

        let random_address = Pubkey::new_unique();
        let mut asd = AccountSharedData::new(1_000_000, 10, &random_address);
        ctx.set_account(&random_address, &asd);
        call_contract(&mut ctx, &[AccountMeta::new(random_address, false)])
            .await
            .expect_error((0, ProgramError::Custom(0)));
    }

    #[tokio::test]
    async fn test_pda_validation() {
        let bump: u32 = 0;

        fn validation_test_contract(
            program_id: &Pubkey,
            accounts: &[AccountInfo],
            instruction_data: &[u8],
        ) -> ProgramResult {
            let expected_bump: u8 = instruction_data[0];
            let mut bump: u8 = 0;
            super::validate(&mut [(
                &accounts[0],
                &mut [super::Validation::Pda {
                    seeds: &[b"escrow"],
                    program: &crate::ID,
                    ret_bump: Some(&mut bump),
                }],
            )])?;
            msg!("{:?}", bump);
            // If bump was not updated as expected, we throw an error as well.
            if bump != expected_bump {
                return Err(ProgramError::Custom(0));
            }
            Ok(())
        }
        let mut program_test =
            ProgramTest::new("dummy", crate::ID, processor!(validation_test_contract));
        let mut ctx = program_test.start_with_context().await;

        let (pda, bump) = Pubkey::find_program_address(&[b"escrow"], &crate::ID);
        call_contract_with_data(&mut ctx, &[AccountMeta::new(pda, false)], vec![bump])
            .await
            .expect_success();
    }

    #[tokio::test]
    async fn test_pda_validation_fail() {
        let mut ctx = context_with_validation!((
            0,
            [super::Validation::Pda {
                seeds: &[b"escrow"],
                program: &crate::ID,
                ret_bump: None
            },]
        ));

        let (pda, _) = Pubkey::find_program_address(&[b"bad"], &crate::ID);
        call_contract(&mut ctx, &[AccountMeta::new(pda, false)])
            .await
            .expect_error((0, ProgramError::Custom(0)));
    }

    #[tokio::test]
    async fn test_init_ata() {
        use spl_associated_token_account::get_associated_token_address_with_program_id;
        fn validation_test_contract(
            program_id: &Pubkey,
            accounts: &[AccountInfo],
            instruction_data: &[u8],
        ) -> ProgramResult {
            let mut bump: u8 = 0;

            super::validate(&mut [(
                &accounts[0],
                &mut [super::Validation::InitAta {
                    payer: &accounts[1].key,
                    token_program: &spl_token::ID,
                    authority: &accounts[2].key,
                    mint: accounts[3].key,
                    accounts: accounts,
                }],
            )])?;
            Ok(())
        }
        let mut program_test =
            ProgramTest::new("dummy", crate::ID, processor!(validation_test_contract));
        let mut ctx = program_test.start_with_context().await;
        let mut client = ctx.banks_client.clone();
        let mint_kp = deploy_spl_token(&mut ctx, 9).await;
        let alice = Pubkey::new_unique();
        let alice_ata =
            get_associated_token_address_with_program_id(&alice, &mint_kp.pubkey(), &spl_token::ID);
        let payer = ctx.payer.pubkey().clone();

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
        let ata_data: Account = client.get_packed_account_data(alice_ata).await.unwrap();
        assert_eq!(ata_data.owner, alice);
        assert_eq!(ata_data.mint, mint_kp.pubkey());
    }
}
