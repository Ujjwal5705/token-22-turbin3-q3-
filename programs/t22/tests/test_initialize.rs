use anchor_lang::{InstructionData, ToAccountMetas};
use litesvm::LiteSVM;
use solana_instruction::Instruction;
use solana_keypair::Keypair;
use solana_signer::Signer;
use solana_transaction::Transaction;
use t22new::extension::{BaseStateWithExtensions, ExtensionType, StateWithExtensions};
use t22new::state::Mint as MintState;

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
fn initialize_mint_stacks_all_four_extensions() {
    let (mut svm, payer) = setup();
    let mint = Keypair::new();

    let decimals: u8 = 6;
    let transfer_fee_basis_points: u16 = 100; // 1%
    let maximum_fee: u64 = 1_000_000;

    let accounts = accounts::InitializeMint {
        payer: payer.pubkey(),
        mint: mint.pubkey(),
        token_program: t22new::ID,
        system_program: solana_system_interface::program::ID,
    };

    let ix = Instruction {
        program_id: PROGRAM_ID,
        accounts: accounts.to_account_metas(None),
        data: instruction::InitializeMint {
            decimals,
            transfer_fee_basis_points,
            maximum_fee,
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
    assert!(result.is_ok(), "initialize_mint failed: {:?}", result.err());

    let mint_account = svm.get_account(&mint.pubkey()).unwrap();
    let state = StateWithExtensions::<MintState>::unpack(&mint_account.data).unwrap();

    let extensions = state.get_extension_types().unwrap();
    assert!(extensions.contains(&ExtensionType::MetadataPointer));
    assert!(extensions.contains(&ExtensionType::TransferFeeConfig));
    assert!(extensions.contains(&ExtensionType::DefaultAccountState));
    assert!(extensions.contains(&ExtensionType::MintCloseAuthority));

    assert_eq!(state.base.decimals, decimals);
    assert!(state.base.is_initialized);
}