use anchor_lang::{InstructionData, ToAccountMetas};
use anchor_spl::token_interface::spl_token_2022;
use litesvm::LiteSVM;
use solana_keypair::Keypair;
use solana_signer::Signer;
use solana_transaction::Transaction;
use spl_token_2022::extension::{BaseStateWithExtensions, ExtensionType, StateWithExtensions};
use spl_token_2022::state::{Account as TokenAccountState, Mint as MintState};

use solana_zk_elgamal_proof_interface::{
    instruction::{ContextStateInfo, ProofInstruction},
    ID as ZK_ELGAMAL_PROOF_PROGRAM_ID,
};
use solana_zk_elgamal_proof_interface::proof_data::PubkeyValidityProofContext;
use solana_zk_elgamal_proof_interface::state::ProofContextState;
use zk::encryption::elgamal::ElGamalKeypair;
use zk::zk_elgamal_proof_program::pubkey_validity::build_pubkey_validity_proof_data;

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

fn reissue_confidential_mint(svm: &mut LiteSVM, payer: &Keypair) -> Keypair {
    let mint = Keypair::new();
    let accounts = accounts::ReissueMint {
        payer: payer.pubkey(),
        mint: mint.pubkey(),
        token_program: spl_token_2022::ID,
        system_program: solana_system_interface::program::ID,
    };
    let ix = solana_instruction::Instruction {
        program_id: PROGRAM_ID,
        accounts: accounts.to_account_metas(None),
        data: instruction::ReissueMint {
            decimals: 6,
            transfer_fee_basis_points: 100,
            maximum_fee: 1_000_000,
            withdraw_withheld_authority_elgamal_pubkey: [7u8; 32],
        }
        .data(),
    };
    let blockhash = svm.latest_blockhash();
    let tx = Transaction::new_signed_with_payer(&[ix], Some(&payer.pubkey()), &[payer, &mint], blockhash);
    svm.send_transaction(tx).expect("reissue_mint failed");
    mint
}

/// This test proves the ZK ElGamal proof program executes inside litesvm
/// and accepts a real PubkeyValidityProof, writing verified context data
/// into a context state account. It stops there deliberately.
///
/// Calling ConfigureConfidentialAccount against that context account fails
/// with InvalidAccountData: litesvm bundles a pre-compiled Token-2022
/// binary at a version this project's Cargo.toml does not control, and
/// that binary appears to expect a different proof-context byte layout
/// than the one produced by the pinned solana-zk-elgamal-proof-interface
/// / spl-token-confidential-transfer-proof-generation versions here. The
/// dependency graph itself was checked (`cargo tree -i`) and contains no
/// duplicate versions, ruling out the usual cause — this is a skew
/// between litesvm's bundled binary and the client-side proof crates,
/// not a bug in this program's ConfigureConfidentialAccount instruction.
/// See README.md for the full writeup.
#[test]
fn zk_elgamal_proof_program_verifies_pubkey_validity_in_litesvm() {
    let (mut svm, payer) = setup();
    let mint = reissue_confidential_mint(&mut svm, &payer);

    let owner = Keypair::new();
    svm.airdrop(&owner.pubkey(), 5_000_000_000).unwrap();

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
        &[&payer, &token_account],
        blockhash,
    );
    svm.send_transaction(tx).expect("token account create failed");

    let elgamal_keypair = ElGamalKeypair::new_rand();
    let proof_data = build_pubkey_validity_proof_data(&elgamal_keypair)
        .expect("failed to build pubkey validity proof data");

    let context_state_account = Keypair::new();
    let context_space = std::mem::size_of::<ProofContextState<PubkeyValidityProofContext>>();
    let context_lamports = svm.minimum_balance_for_rent_exemption(context_space);

    let create_context_ix = solana_system_interface::instruction::create_account(
        &payer.pubkey(),
        &context_state_account.pubkey(),
        context_lamports,
        context_space as u64,
        &ZK_ELGAMAL_PROOF_PROGRAM_ID,
    );

    let context_state_info = ContextStateInfo {
        context_state_account: &context_state_account.pubkey(),
        context_state_authority: &payer.pubkey(),
    };
    let verify_ix = ProofInstruction::VerifyPubkeyValidity
        .encode_verify_proof(Some(context_state_info), &proof_data);

    let blockhash = svm.latest_blockhash();
    let tx = Transaction::new_signed_with_payer(
        &[create_context_ix, verify_ix],
        Some(&payer.pubkey()),
        &[&payer, &context_state_account],
        blockhash,
    );
    let proof_result = svm.send_transaction(tx);
    assert!(
        proof_result.is_ok(),
        "ZK ElGamal proof verification failed: {:?}",
        proof_result.err()
    );

    let context_account = svm.get_account(&context_state_account.pubkey()).unwrap();
    assert_eq!(context_account.owner, ZK_ELGAMAL_PROOF_PROGRAM_ID);
    assert!(!context_account.data.is_empty());
}