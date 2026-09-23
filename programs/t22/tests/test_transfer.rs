use anchor_lang::{InstructionData, ToAccountMetas};
use anchor_spl::token_interface::spl_token_2022;
use litesvm::LiteSVM;
use solana_keypair::Keypair;
use solana_signer::Signer;
use solana_transaction::Transaction;
use spl_token_2022::extension::{
    transfer_fee::TransferFeeConfig, BaseStateWithExtensions, ExtensionType, StateWithExtensions,
};
use spl_token_2022::state::{Account as TokenAccountState, Mint as MintState};

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

fn create_and_fund_token_account(
    svm: &mut LiteSVM,
    payer: &Keypair,
    mint: &Keypair,
    owner: &Keypair,
    amount: u64,
) -> Keypair {
    let token_account = Keypair::new();

    // Size the account for whatever extensions the mint requires on it
    // (e.g. TransferFeeAmount, since the mint carries TransferFeeConfig).
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

    // Thaw via our own Task 4 instruction — freeze authority (payer)
    // clears this one account, mint-level default state untouched.
    let unfreeze_accounts = accounts::UnfreezeAccount {
        freeze_authority: payer.pubkey(),
        token_account: token_account.pubkey(),
        mint: mint.pubkey(),
        token_program: spl_token_2022::ID,
    };
    let unfreeze_ix = solana_instruction::Instruction {
        program_id: PROGRAM_ID,
        accounts: unfreeze_accounts.to_account_metas(None),
        data: instruction::UnfreezeAccount {}.data(),
    };
    let blockhash = svm.latest_blockhash();
    let tx = Transaction::new_signed_with_payer(&[unfreeze_ix], Some(&payer.pubkey()), &[payer], blockhash);
    svm.send_transaction(tx).expect("unfreeze failed");

    if amount > 0 {
        let mint_to_ix = spl_token_2022::instruction::mint_to(
            &spl_token_2022::ID,
            &mint.pubkey(),
            &token_account.pubkey(),
            &payer.pubkey(), // mint authority
            &[],
            amount,
        )
        .unwrap();
        let blockhash = svm.latest_blockhash();
        let tx = Transaction::new_signed_with_payer(&[mint_to_ix], Some(&payer.pubkey()), &[payer], blockhash);
        svm.send_transaction(tx).expect("mint_to failed");
    }

    token_account
}

#[test]
fn transfer_with_fee_withholds_the_live_epoch_fee() {
    let (mut svm, payer) = setup();

    let decimals = 6u8;
    let fee_bps = 100u16; // 1%
    let max_fee = 1_000_000u64;
    let mint = create_mint(&mut svm, &payer, decimals, fee_bps, max_fee);

    let source_owner = Keypair::new();
    let dest_owner = Keypair::new();

    let starting_amount = 1_000_000u64;
    let source = create_and_fund_token_account(&mut svm, &payer, &mint, &source_owner, starting_amount);
    let destination = create_and_fund_token_account(&mut svm, &payer, &mint, &dest_owner, 0);

    // Compute the fee independently, the same way the program is supposed
    // to (StateWithExtensions + calculate_epoch_fee), so the test isn't
    // just trusting the program's own arithmetic.
    let transfer_amount = 100_000u64;
    let expected_fee = {
        let mint_account = svm.get_account(&mint.pubkey()).unwrap();
        let state = StateWithExtensions::<MintState>::unpack(&mint_account.data).unwrap();
        let config = state.get_extension::<TransferFeeConfig>().unwrap();
        // A freshly constructed LiteSVM instance starts at epoch 0.
        config.calculate_epoch_fee(0, transfer_amount).unwrap()
    };

    let accounts = accounts::TransferWithFee {
        authority: source_owner.pubkey(),
        source: source.pubkey(),
        mint: mint.pubkey(),
        destination: destination.pubkey(),
        token_program: spl_token_2022::ID,
    };

    let ix = solana_instruction::Instruction {
        program_id: PROGRAM_ID,
        accounts: accounts.to_account_metas(None),
        data: instruction::TransferWithFee {
            amount: transfer_amount,
            decimals,
        }
        .data(),
    };

    let blockhash = svm.latest_blockhash();
    let tx = Transaction::new_signed_with_payer(
        &[ix],
        Some(&payer.pubkey()),
        &[&payer, &source_owner],
        blockhash,
    );

    let result = svm.send_transaction(tx);
    assert!(result.is_ok(), "transfer_with_fee failed: {:?}", result.err());

    let source_account = svm.get_account(&source.pubkey()).unwrap();
    let source_state = StateWithExtensions::<TokenAccountState>::unpack(&source_account.data).unwrap();
    assert_eq!(source_state.base.amount, starting_amount - transfer_amount);

    let dest_account = svm.get_account(&destination.pubkey()).unwrap();
    let dest_state = StateWithExtensions::<TokenAccountState>::unpack(&dest_account.data).unwrap();
    assert_eq!(dest_state.base.amount, transfer_amount - expected_fee);
}