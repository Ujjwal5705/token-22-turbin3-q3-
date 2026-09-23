use anchor_lang::{InstructionData, ToAccountMetas};
use anchor_spl::token_interface::spl_token_2022;
use litesvm::LiteSVM;
use solana_keypair::Keypair;
use solana_signer::Signer;
use solana_transaction::Transaction;
use spl_token_2022::extension::{BaseStateWithExtensions, ExtensionType, StateWithExtensions};
use spl_token_2022::state::{Account as TokenAccountState, AccountState, Mint as MintState};

use t22::{accounts, instruction, ID as PROGRAM_ID};

fn setup() -> (LiteSVM, Keypair) {
    let mut svm = LiteSVM::new();
    let payer = Keypair::new();
    svm.airdrop(&payer.pubkey(), 10_000_000_000).unwrap();

    let program_bytes = include_bytes!("../../../target/deploy/t22.so");
    svm.add_program(PROGRAM_ID, program_bytes)
        .expect("failed to load t22 program into litesvm");

    (svm, payer)
}

fn create_mint(svm: &mut LiteSVM, payer: &Keypair, decimals: u8, fee_bps: u16, max_fee: u64) -> Keypair {
    let mint = Keypair::new();

    let accounts = accounts::InitializeMint {
        payer: payer.pubkey(),
        mint: mint.pubkey(),
        token_program: spl_token_2022::ID,
        system_program: solana_system_interface::program::ID,
    };

    let ix = solana_instruction::Instruction {
        program_id: PROGRAM_ID,
        accounts: accounts.to_account_metas(None),
        data: instruction::InitializeMint {
            decimals,
            transfer_fee_basis_points: fee_bps,
            maximum_fee: max_fee,
        }
        .data(),
    };

    let blockhash = svm.latest_blockhash();
    let tx = Transaction::new_signed_with_payer(&[ix], Some(&payer.pubkey()), &[payer, &mint], blockhash);
    svm.send_transaction(tx).expect("mint init failed");

    mint
}

/// Creates a token account WITHOUT thawing it — unlike the Task 2 test
/// helper, this leaves the account frozen so we can test our own
/// unfreeze instruction against it.
fn create_frozen_token_account(svm: &mut LiteSVM, payer: &Keypair, mint: &Keypair, owner: &Keypair) -> Keypair {
    let token_account = Keypair::new();

    let mint_account = svm.get_account(&mint.pubkey()).unwrap();
    let mint_state = StateWithExtensions::<MintState>::unpack(&mint_account.data).unwrap();
    let mint_extensions = mint_state.get_extension_types().unwrap();
    let required = ExtensionType::get_required_init_account_extensions(&mint_extensions);
    let space = ExtensionType::try_calculate_account_len::<TokenAccountState>(&required).unwrap();
    let lamports = svm.minimum_balance_for_rent_exemption(space);

    let create_ix = solana_system_interface::instruction::create_account(
        &payer.pubkey(),
        &token_account.pubkey(),
        lamports,
        space as u64,
        &spl_token_2022::ID,
    );

    let init_ix = spl_token_2022::instruction::initialize_account3(
        &spl_token_2022::ID,
        &token_account.pubkey(),
        &mint.pubkey(),
        &owner.pubkey(),
    )
    .unwrap();

    let blockhash = svm.latest_blockhash();
    let tx = Transaction::new_signed_with_payer(
        &[create_ix, init_ix],
        Some(&payer.pubkey()),
        &[payer, &token_account],
        blockhash,
    );
    svm.send_transaction(tx).expect("token account create failed");

    token_account
}

#[test]
fn unfreeze_account_thaws_one_account_only() {
    let (mut svm, payer) = setup();
    let mint = create_mint(&mut svm, &payer, 6, 100, 1_000_000);

    let owner_a = Keypair::new();
    let owner_b = Keypair::new();
    let account_a = create_frozen_token_account(&mut svm, &payer, &mint, &owner_a);
    let account_b = create_frozen_token_account(&mut svm, &payer, &mint, &owner_b);

    // Sanity check: both born frozen, per Task 1's DefaultAccountState.
    for acct in [&account_a, &account_b] {
        let data = svm.get_account(&acct.pubkey()).unwrap();
        let state = StateWithExtensions::<TokenAccountState>::unpack(&data.data).unwrap();
        assert_eq!(state.base.state, AccountState::Frozen);
    }

    // Thaw only account_a.
    let accounts = accounts::UnfreezeAccount {
        freeze_authority: payer.pubkey(),
        token_account: account_a.pubkey(),
        mint: mint.pubkey(),
        token_program: spl_token_2022::ID,
    };
    let ix = solana_instruction::Instruction {
        program_id: PROGRAM_ID,
        accounts: accounts.to_account_metas(None),
        data: instruction::UnfreezeAccount {}.data(),
    };
    let blockhash = svm.latest_blockhash();
    let tx = Transaction::new_signed_with_payer(&[ix], Some(&payer.pubkey()), &[&payer], blockhash);
    let result = svm.send_transaction(tx);
    assert!(result.is_ok(), "unfreeze failed: {:?}", result.err());

    // account_a is now thawed...
    let a_data = svm.get_account(&account_a.pubkey()).unwrap();
    let a_state = StateWithExtensions::<TokenAccountState>::unpack(&a_data.data).unwrap();
    assert_eq!(a_state.base.state, AccountState::Initialized);

    // ...but account_b is untouched, and the mint's DefaultAccountState is
    // unchanged, so a brand new account is still born frozen.
    let b_data = svm.get_account(&account_b.pubkey()).unwrap();
    let b_state = StateWithExtensions::<TokenAccountState>::unpack(&b_data.data).unwrap();
    assert_eq!(b_state.base.state, AccountState::Frozen);

    let account_c = create_frozen_token_account(&mut svm, &payer, &mint, &Keypair::new());
    let c_data = svm.get_account(&account_c.pubkey()).unwrap();
    let c_state = StateWithExtensions::<TokenAccountState>::unpack(&c_data.data).unwrap();
    assert_eq!(c_state.base.state, AccountState::Frozen);
}

#[test]
fn unfreeze_fails_with_wrong_authority() {
    let (mut svm, payer) = setup();
    let mint = create_mint(&mut svm, &payer, 6, 100, 1_000_000);
    let account = create_frozen_token_account(&mut svm, &payer, &mint, &Keypair::new());

    let impostor = Keypair::new();
    svm.airdrop(&impostor.pubkey(), 1_000_000_000).unwrap();

    let accounts = accounts::UnfreezeAccount {
        freeze_authority: impostor.pubkey(),
        token_account: account.pubkey(),
        mint: mint.pubkey(),
        token_program: spl_token_2022::ID,
    };
    let ix = solana_instruction::Instruction {
        program_id: PROGRAM_ID,
        accounts: accounts.to_account_metas(None),
        data: instruction::UnfreezeAccount {}.data(),
    };
    let blockhash = svm.latest_blockhash();
    let tx = Transaction::new_signed_with_payer(&[ix], Some(&impostor.pubkey()), &[&impostor], blockhash);
    let result = svm.send_transaction(tx);

    assert!(result.is_err(), "unfreeze should fail when signer is not the freeze authority");
}