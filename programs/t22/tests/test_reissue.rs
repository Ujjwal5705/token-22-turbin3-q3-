use anchor_lang::{InstructionData, ToAccountMetas};
use anchor_spl::token_interface::spl_token_2022;
use litesvm::LiteSVM;
use solana_keypair::Keypair;
use solana_signer::Signer;
use solana_transaction::Transaction;
use spl_token_2022::extension::{BaseStateWithExtensions, ExtensionType, StateWithExtensions};
use spl_token_2022::state::Mint as MintState;

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

#[test]
fn reissue_mint_carries_forward_all_extensions_plus_two_new_ones() {
    let (mut svm, payer) = setup();
    let mint = Keypair::new();

    let decimals = 6u8;
    let fee_bps = 100u16;
    let max_fee = 1_000_000u64;

    let accounts = accounts::ReissueMint {
        payer: payer.pubkey(),
        mint: mint.pubkey(),
        token_program: spl_token_2022::ID,
        system_program: solana_system_interface::program::ID,
    };

    let elgamal_pubkey = [7u8; 32]; // placeholder key for this test

    let ix = solana_instruction::Instruction {
        program_id: PROGRAM_ID,
        accounts: accounts.to_account_metas(None),
        data: instruction::ReissueMint {
            decimals,
            transfer_fee_basis_points: fee_bps,
            maximum_fee: max_fee,
            withdraw_withheld_authority_elgamal_pubkey: elgamal_pubkey,
        }
        .data(),
    };

    let blockhash = svm.latest_blockhash();
    let tx = Transaction::new_signed_with_payer(
        &[ix],
        Some(&payer.pubkey()),
        &[&payer, &mint],
        blockhash,
    );

    let result = svm.send_transaction(tx);
    assert!(result.is_ok(), "reissue_mint failed: {:?}", result.err());

    let mint_account = svm.get_account(&mint.pubkey()).unwrap();
    let state = StateWithExtensions::<MintState>::unpack(&mint_account.data).unwrap();
    let extensions = state.get_extension_types().unwrap();

    // The original four, carried forward.
    assert!(extensions.contains(&ExtensionType::MetadataPointer));
    assert!(extensions.contains(&ExtensionType::TransferFeeConfig));
    assert!(extensions.contains(&ExtensionType::DefaultAccountState));
    assert!(extensions.contains(&ExtensionType::MintCloseAuthority));

    // The two new ones from the scenario.
    assert!(extensions.contains(&ExtensionType::PermanentDelegate));
    assert!(extensions.contains(&ExtensionType::ConfidentialTransferMint));
    assert!(extensions.contains(&ExtensionType::ConfidentialTransferFeeConfig));

    assert_eq!(extensions.len(), 7, "expected exactly seven extensions, got {:?}", extensions);
    assert_eq!(state.base.decimals, decimals);

    // Manual approval: auto_approve_new_accounts must be false.
    use spl_token_2022::extension::confidential_transfer::ConfidentialTransferMint;
    let ct_config = state.get_extension::<ConfidentialTransferMint>().unwrap();
    assert_eq!(
        bool::from(ct_config.auto_approve_new_accounts),
        false,
        "expected manual approve_policy (auto_approve_new_accounts = false)"
    );

    // Permanent delegate is set to payer.
    use spl_token_2022::extension::permanent_delegate::PermanentDelegate;
    use anchor_lang::solana_program::program_option::COption;
    let pd_config = state.get_extension::<PermanentDelegate>().unwrap();
    let delegate: COption<anchor_lang::prelude::Pubkey> = pd_config.delegate.into();
    assert_eq!(delegate, COption::Some(payer.pubkey()));
}