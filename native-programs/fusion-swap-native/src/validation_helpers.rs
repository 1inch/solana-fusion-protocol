#![allow(clippy::unit_arg)]
use solana_program::{
    account_info::AccountInfo, entrypoint::ProgramResult, program::invoke,
    program_error::ProgramError, pubkey::Pubkey,
};

use spl_associated_token_account::instruction as spl_ata_instruction;
use spl_token_2022::{extension::StateWithExtensions, state::Account, state::Mint};

use crate::error::FusionError;
use Result::*;

#[macro_export]
macro_rules! require {
    ($x:expr, $e: expr) => {{
        if !($x) {
            return Err($e);
        };
    }};
}

pub fn assert_signer(account_info: &AccountInfo) -> ProgramResult {
    Ok(require!(
        account_info.is_signer,
        FusionError::ConstraintSigner.into()
    ))
}

pub fn assert_mint(account_info: &AccountInfo) -> Result<Mint, ProgramError> {
    require!(
        is_token_program(account_info.owner),
        FusionError::ConstraintTokenMint.into()
    );

    // Here we use spl-token-2022 library to unpack the Mint data because of backward compatibility.
    StateWithExtensions::<Mint>::unpack(&account_info.data.borrow())
        .map_err(|_| FusionError::ConstraintTokenMint.into())
        .map(|s| s.base)
}

pub fn assert_writable(account_info: &AccountInfo) -> ProgramResult {
    Ok(require!(
        account_info.is_writable,
        FusionError::AccountNotWritable.into()
    ))
}

pub fn assert_token_account(
    account_info: &AccountInfo,
    mint: &Pubkey,
    opt_authority: Option<&Pubkey>,
    opt_token_program: Option<&Pubkey>,
) -> ProgramResult {
    // Decode account data
    let data: &[u8] = &mut account_info.data.borrow();

    // Unpack the data using spl-2022 account deserialization because of backward compatibility with spl-token.
    let acc_data = StateWithExtensions::<Account>::unpack(data)?;

    // Check mint
    require!(
        acc_data.base.mint == *mint,
        FusionError::ConstraintTokenMint.into()
    );
    // Check token account owner
    if let Some(exp_authority) = opt_authority {
        require!(
            acc_data.base.owner == *exp_authority,
            FusionError::ConstraintTokenOwner.into()
        );
        require!(
            spl_associated_token_account::get_associated_token_address_with_program_id(
                &acc_data.base.owner,
                &acc_data.base.mint,
                account_info.owner
            ) == *account_info.key,
            FusionError::AccountNotAssociatedTokenAccount.into()
        );
    };
    if let Some(token_program) = opt_token_program {
        // Check token program of the account by checking
        // the solana account owner
        require!(
            *account_info.owner == *token_program,
            FusionError::ConstraintMintTokenProgram.into()
        );
    }
    Ok(())
}

pub fn assert_pda(
    account_info: &AccountInfo,
    seeds: &[&[u8]],
    program: &Pubkey,
) -> Result<u8, ProgramError> {
    let (pda, bump) = Pubkey::try_find_program_address(seeds, program)
        .ok_or::<FusionError>(FusionError::ConstraintSeeds)?;
    require!(
        *account_info.key == pda,
        FusionError::ConstraintSeeds.into()
    );
    Ok(bump)
}

pub fn assert_key(account_info: &AccountInfo, exp_pubkey: &Pubkey) -> ProgramResult {
    Ok(require!(
        *account_info.key == *exp_pubkey,
        FusionError::ConstraintAddress.into()
    ))
}

#[inline(always)]
fn is_token_program(key: &Pubkey) -> bool {
    *key == spl_token::ID || *key == spl_token_2022::ID
}

pub fn assert_token_program(account_info: &AccountInfo) -> ProgramResult {
    Ok(require!(
        is_token_program(account_info.key),
        FusionError::ConstraintAddress.into()
    ))
}

pub fn init_ata_with_address_check(
    account_info: &AccountInfo,
    payer: &Pubkey,
    mint: &Pubkey,
    authority: &Pubkey,
    token_program: &Pubkey,
    accounts: &[AccountInfo],
    // The following accounts should be present in this slice, but their order does not matter.
    // * [writable] ATA address
    // * [signer] Payer
    // * Owner pubkey
    // * Mint pubkey
    // * System progam
    // * SPL token program
) -> ProgramResult {
    // Ensure the account does not exist already.
    require!(
        account_info.data_is_empty()
            && account_info.lamports() == 0
            && *account_info.owner == solana_program::system_program::ID,
        ProgramError::AccountAlreadyInitialized
    );
    // Validate the account address
    let ata = spl_associated_token_account::get_associated_token_address_with_program_id(
        authority,
        mint,
        token_program,
    );

    require!(
        ata == *account_info.key,
        FusionError::AccountNotAssociatedTokenAccount.into()
    );

    // Create the associated token account
    let create_ix =
        spl_ata_instruction::create_associated_token_account(payer, authority, mint, token_program);
    invoke(&create_ix, &accounts[0..6])
}

#[cfg(test)]
mod tests {
    use super::*;

    use solana_program::{
        account_info::AccountInfo, entrypoint::ProgramResult, instruction::AccountMeta,
        instruction::Instruction, program_error::ProgramError, program_pack::Pack, pubkey::Pubkey,
    };
    use solana_program_test::{
        processor, tokio, BanksClientError, ProgramTest, ProgramTestContext,
    };
    use solana_sdk::{
        account::{AccountSharedData, WritableAccount},
        signature::Signer,
        signer::keypair::Keypair,
        system_instruction,
        transaction::Transaction,
        transaction::TransactionError,
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
        let create_spl_acc_ix = spl_ata_instruction::create_associated_token_account(
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

    pub async fn initialize_token_account(
        ctx: &mut ProgramTestContext,
        mint_pubkey: &Pubkey,
        owner: &Pubkey,
    ) -> Keypair {
        let account_keypair = Keypair::new();

        let create_spl_acc_ix = system_instruction::create_account(
            &ctx.payer.pubkey(),
            &account_keypair.pubkey(),
            1_000_000_000,
            Account::LEN as u64,
            &spl_token::ID,
        );

        let initialize_acc_ix: Instruction = spl_token::instruction::initialize_account(
            &spl_token::ID,
            &account_keypair.pubkey(),
            mint_pubkey,
            owner,
        )
        .unwrap();

        let signers: Vec<&Keypair> = vec![&ctx.payer, &account_keypair];

        let client = &mut ctx.banks_client;
        client
            .process_transaction(Transaction::new_signed_with_payer(
                &[create_spl_acc_ix, initialize_acc_ix],
                Some(&ctx.payer.pubkey()),
                &signers,
                ctx.last_blockhash,
            ))
            .await
            .unwrap();
        account_keypair
    }

    pub async fn initialize_spl2022_associated_token_account(
        ctx: &mut ProgramTestContext,
        mint_pubkey: &Pubkey,
        owner: &Pubkey,
    ) -> Pubkey {
        let ata = spl_associated_token_account::get_associated_token_address_with_program_id(
            owner,
            mint_pubkey,
            &spl_token_2022::ID,
        );

        let create_spl_acc_ix = spl_ata_instruction::create_associated_token_account(
            &ctx.payer.pubkey(),
            owner,
            mint_pubkey,
            &spl_token_2022::ID,
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
        let asd = AccountSharedData::new_data(1_000_000, &vec![0; 10], owner).unwrap();
        ctx.set_account(&key, &asd);
        key
    }

    pub async fn deploy_spl2022_token(ctx: &mut ProgramTestContext, decimals: u8) -> Keypair {
        use spl_token_2022::extension::ExtensionType;
        use spl_token_2022::{
            instruction as spl2022_instruction, state::Mint as SPL2022_Mint,
            ID as spl2022_program_id,
        };
        // create mint account
        let mint_keypair = Keypair::new();
        let account_size = ExtensionType::try_calculate_account_len::<SPL2022_Mint>(&[]).unwrap();
        let create_mint_acc_ix = system_instruction::create_account(
            &ctx.payer.pubkey(),
            &mint_keypair.pubkey(),
            1_000_000_000, // Some lamports to pay rent
            account_size as u64,
            &spl2022_program_id,
        );

        // initialize mint account
        let initialize_mint_ix: Instruction = spl2022_instruction::initialize_mint(
            &spl2022_program_id,
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
            .expect_error((0, FusionError::ConstraintAddress.into()));
    }

    #[tokio::test]
    async fn test_token_program_validation() {
        let mut ctx = context_with_validation!(|x| assert_token_program(x));
        call_contract(&mut ctx, &[AccountMeta::new(spl_token::ID, false)])
            .await
            .expect_success();
    }

    #[tokio::test]
    async fn test_token_program_validation_2022() {
        let mut ctx = context_with_validation!(|x| assert_token_program(x));
        call_contract(&mut ctx, &[AccountMeta::new(spl_token_2022::ID, false)])
            .await
            .expect_success();
    }

    #[tokio::test]
    async fn test_token_program_validation_fail() {
        let mut ctx = context_with_validation!(|x| assert_token_program(x));
        call_contract(&mut ctx, &[AccountMeta::new(Pubkey::new_unique(), false)])
            .await
            .expect_error((0, FusionError::ConstraintAddress.into()));
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
        .expect_error((0, FusionError::AccountNotWritable.into()));
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
    async fn test_mint_owner_validation_fail() {
        let mut ctx = context_with_validation!(|x| assert_mint(x));
        let mint_kp = deploy_spl_token(&mut ctx, 9).await;
        let mut client = ctx.banks_client.clone();

        // Get mint account
        let mint_acount = client.get_account(mint_kp.pubkey()).await.unwrap().unwrap();

        // Change its owner to something random
        let mut asd = AccountSharedData::from(mint_acount);
        asd.set_owner(Pubkey::new_unique());
        ctx.set_account(&mint_kp.pubkey(), &asd);

        call_contract(&mut ctx, &[AccountMeta::new(mint_kp.pubkey(), false)])
            .await
            .expect_error((0, FusionError::ConstraintTokenMint.into()));
    }

    #[tokio::test]
    async fn test_mint_2022_validation() {
        let mut ctx = context_with_validation!(|x| assert_mint(x));
        let mint_kp = deploy_spl2022_token(&mut ctx, 9).await;
        call_contract(&mut ctx, &[AccountMeta::new(mint_kp.pubkey(), false)])
            .await
            .expect_success();
    }

    #[tokio::test]
    async fn test_mint_data_validation_fail() {
        let mut ctx = context_with_validation!(|x| assert_mint(x));
        let bad_mint = create_account_with_owner(&mut ctx, &spl_token::ID);
        call_contract(&mut ctx, &[AccountMeta::new(bad_mint, false)])
            .await
            .expect_error((0, FusionError::ConstraintTokenMint.into()));
    }

    // A test contract that is used in couple of following tests.
    fn validation_test_contract_for_token_account_validation_test(
        _: &Pubkey,
        accounts: &[AccountInfo],
        _: &[u8],
    ) -> ProgramResult {
        assert_token_account(
            &accounts[0],
            accounts[1].key,
            Some(accounts[2].key),
            Some(&spl_token::ID),
        )?;
        Ok(())
    }

    #[tokio::test]
    async fn test_token_account_validation() {
        let program_test = ProgramTest::new(
            "dummy",
            crate::ID,
            processor!(validation_test_contract_for_token_account_validation_test),
        );
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
        let program_test = ProgramTest::new(
            "dummy",
            crate::ID,
            processor!(validation_test_contract_for_token_account_validation_test),
        );
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
        .expect_error((0, FusionError::ConstraintTokenOwner.into()));
    }

    #[tokio::test]
    async fn test_ata_validation_fail_for_token_account() {
        let program_test = ProgramTest::new(
            "dummy",
            crate::ID,
            processor!(validation_test_contract_for_token_account_validation_test),
        );
        let mut ctx = program_test.start_with_context().await;

        let user_pk = Pubkey::new_unique();
        let mint_kp = deploy_spl_token(&mut ctx, 9).await;
        let non_ata = initialize_token_account(&mut ctx, &mint_kp.pubkey(), &user_pk).await;
        call_contract(
            &mut ctx,
            &[
                AccountMeta::new(non_ata.pubkey(), false),
                AccountMeta::new(mint_kp.pubkey(), false),
                AccountMeta::new(user_pk, false),
            ],
        )
        .await
        .expect_error((0, FusionError::AccountNotAssociatedTokenAccount.into()));
    }

    #[tokio::test]
    async fn test_token2022_account_validation() {
        fn validation_test_contract(
            _: &Pubkey,
            accounts: &[AccountInfo],
            _: &[u8],
        ) -> ProgramResult {
            assert_token_account(
                &accounts[0],
                accounts[1].key,
                Some(accounts[2].key),
                Some(&spl_token_2022::ID),
            )?;
            Ok(())
        }
        let program_test =
            ProgramTest::new("dummy", crate::ID, processor!(validation_test_contract));
        let mut ctx = program_test.start_with_context().await;

        let user_pk = Pubkey::new_unique();
        let mint_kp = deploy_spl2022_token(&mut ctx, 9).await;
        let ata =
            initialize_spl2022_associated_token_account(&mut ctx, &mint_kp.pubkey(), &user_pk)
                .await;
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
            .expect_error((0, FusionError::ConstraintSeeds.into()));
    }

    fn validation_test_contract_for_init_ata(
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

    #[tokio::test]
    async fn test_init_ata() {
        let program_test = ProgramTest::new(
            "dummy",
            crate::ID,
            processor!(validation_test_contract_for_init_ata),
        );
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
                AccountMeta::new_readonly(payer, true),
                AccountMeta::new_readonly(alice, false),
                AccountMeta::new_readonly(mint_kp.pubkey(), false),
                AccountMeta::new_readonly(solana_program::system_program::ID, false),
                AccountMeta::new_readonly(spl_token::ID, false),
                AccountMeta::new_readonly(spl_associated_token_account::ID, false),
            ],
        )
        .await
        .expect_success();

        // Assert ATA attributes.
        let ata_data: Account = client.get_packed_account_data(alice_ata).await.unwrap();
        assert_eq!(ata_data.owner, alice);
        assert_eq!(ata_data.mint, mint_kp.pubkey());
    }

    fn validation_test_contract_for_init_ata_with_accounts_reordered(
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
            &[
                // We jumble up account infos when passing to the
                // function, to ensure that it works despite this.
                accounts[5].clone(),
                accounts[0].clone(),
                accounts[1].clone(),
                accounts[4].clone(),
                accounts[3].clone(),
                accounts[2].clone(),
            ],
        )?;
        Ok(())
    }

    #[tokio::test]
    async fn test_init_ata_with_reordered_accounts() {
        let program_test = ProgramTest::new(
            "dummy",
            crate::ID,
            processor!(validation_test_contract_for_init_ata_with_accounts_reordered),
        );
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
                AccountMeta::new_readonly(payer, true),
                AccountMeta::new_readonly(alice, false),
                AccountMeta::new_readonly(mint_kp.pubkey(), false),
                AccountMeta::new_readonly(solana_program::system_program::ID, false),
                AccountMeta::new_readonly(spl_token::ID, false),
                AccountMeta::new_readonly(spl_associated_token_account::ID, false),
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
        let program_test = ProgramTest::new(
            "dummy",
            crate::ID,
            processor!(validation_test_contract_for_init_ata),
        );
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
        .expect_error((0, FusionError::AccountNotAssociatedTokenAccount.into()));
    }
}
