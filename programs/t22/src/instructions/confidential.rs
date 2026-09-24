use anchor_lang::prelude::*;
use anchor_lang::solana_program::program::invoke;
use anchor_spl::token_interface::{spl_token_2022, TokenInterface};
use spl_token_2022::extension::confidential_transfer::{
    instruction as confidential_instruction, DecryptableBalance,
};
use spl_token_confidential_transfer_proof_extraction::instruction::ProofLocation;

use crate::error::ErrorCode;

/// The length of an AE ciphertext — how a decryptable balance is
/// represented in account data.
pub const AE_CIPHERTEXT_LEN: usize = 36;

/// Task 6, step 1: ConfigureAccount.
///
/// Owner-only, and distinct from ATA creation-by-anyone: creating the
/// underlying token account can be done by anyone paying rent (that's
/// what "creation by anyone" means for an ATA), but opting a specific
/// account into confidential transfers is a decision only the account's
/// owner can make — it commits them to managing an AES key and an ElGamal
/// keypair for that account going forward. Token-2022 enforces this by
/// requiring the owner's signature here, separately from whatever account
/// creation already happened.
///
/// The proof that this account's ElGamal public key is well-formed
/// (PubkeyValidityProof) is generated off-chain and pre-verified into a
/// proof context state account before this instruction runs. This
/// instruction only references that account; it does not itself touch
/// any cryptographic material.
#[derive(Accounts)]
pub struct ConfigureConfidentialAccount<'info> {
    pub owner: Signer<'info>,

    /// CHECK: validated by Token-2022.
    #[account(mut, owner = token_program.key())]
    pub token_account: UncheckedAccount<'info>,

    /// CHECK: validated by Token-2022.
    #[account(owner = token_program.key())]
    pub mint: UncheckedAccount<'info>,

    /// CHECK: a proof context state account already verified by the ZK
    /// ElGamal proof program, holding a PubkeyValidityProof for this
    /// account's ElGamal public key.
    pub proof_context_state: UncheckedAccount<'info>,

    pub token_program: Interface<'info, TokenInterface>,
}

pub fn handle_configure_confidential_account(
    ctx: Context<ConfigureConfidentialAccount>,
    decryptable_zero_balance: [u8; AE_CIPHERTEXT_LEN],
    maximum_pending_balance_credit_counter: u64,
) -> Result<()> {
    let proof_context_state_key = ctx.accounts.proof_context_state.key();
    let proof_location = ProofLocation::ContextStateAccount(&proof_context_state_key);

    let balance = DecryptableBalance::from(decryptable_zero_balance);

    let ixs = confidential_instruction::configure_account(
        &ctx.accounts.token_program.key(),
        &ctx.accounts.token_account.key(),
        &ctx.accounts.mint.key(),
        &balance,
        maximum_pending_balance_credit_counter,
        &ctx.accounts.owner.key(),
        &[],
        proof_location,
    )
    .map_err(|_| error!(ErrorCode::ConfidentialInstructionBuildFailed))?;

    let infos = [
        ctx.accounts.token_account.to_account_info(),
        ctx.accounts.mint.to_account_info(),
        ctx.accounts.owner.to_account_info(),
        ctx.accounts.proof_context_state.to_account_info(),
        ctx.accounts.token_program.to_account_info(),
    ];

    for ix in ixs {
        invoke(&ix, &infos)?;
    }

    msg!(
        "confidential transfers configured for account {} by owner {}",
        ctx.accounts.token_account.key(),
        ctx.accounts.owner.key()
    );
    Ok(())
}

/// Task 6, step 2: DepositConfidentialTokens.
///
/// The one step of the lifecycle that needs no proof — the amount being
/// moved was already public (it's leaving the plaintext balance), so
/// there is nothing to hide yet. It lands in the account's *pending*
/// confidential balance, not the available one; ApplyPendingBalance
/// (step 3) is a separate, required step before it can be spent.
#[derive(Accounts)]
pub struct DepositConfidentialTokens<'info> {
    pub authority: Signer<'info>,

    /// CHECK: validated by Token-2022.
    #[account(mut, owner = token_program.key())]
    pub token_account: UncheckedAccount<'info>,

    /// CHECK: validated by Token-2022.
    #[account(owner = token_program.key())]
    pub mint: UncheckedAccount<'info>,

    pub token_program: Interface<'info, TokenInterface>,
}

pub fn handle_deposit_confidential_tokens(
    ctx: Context<DepositConfidentialTokens>,
    amount: u64,
    decimals: u8,
) -> Result<()> {
    let ix = confidential_instruction::deposit(
        &ctx.accounts.token_program.key(),
        &ctx.accounts.token_account.key(),
        &ctx.accounts.mint.key(),
        amount,
        decimals,
        &ctx.accounts.authority.key(),
        &[],
    )
    .map_err(|_| error!(ErrorCode::ConfidentialInstructionBuildFailed))?;

    invoke(
        &ix,
        &[
            ctx.accounts.token_account.to_account_info(),
            ctx.accounts.mint.to_account_info(),
            ctx.accounts.authority.to_account_info(),
            ctx.accounts.token_program.to_account_info(),
        ],
    )?;

    msg!(
        "deposited {} (decimals {}) into pending confidential balance of {}",
        amount,
        decimals,
        ctx.accounts.token_account.key()
    );
    Ok(())
}

/// Task 6, step 3: ApplyPendingBalance.
///
/// Moves the pending balance into the spendable available balance. The
/// new available-balance ciphertext is supplied by the owner, encrypted
/// under their own AES key — the program has no way to compute this
/// value itself, since it has no access to that key. This is a pass
/// through, not a computation.
#[derive(Accounts)]
pub struct ApplyConfidentialPendingBalance<'info> {
    pub authority: Signer<'info>,

    /// CHECK: validated by Token-2022.
    #[account(mut, owner = token_program.key())]
    pub token_account: UncheckedAccount<'info>,

    pub token_program: Interface<'info, TokenInterface>,
}

pub fn handle_apply_confidential_pending_balance(
    ctx: Context<ApplyConfidentialPendingBalance>,
    expected_pending_balance_credit_counter: u64,
    new_decryptable_available_balance: [u8; AE_CIPHERTEXT_LEN],
) -> Result<()> {
    let balance = DecryptableBalance::from(new_decryptable_available_balance);

    let ix = confidential_instruction::apply_pending_balance(
        &ctx.accounts.token_program.key(),
        &ctx.accounts.token_account.key(),
        expected_pending_balance_credit_counter,
        &balance,
        &ctx.accounts.authority.key(),
        &[],
    )
    .map_err(|_| error!(ErrorCode::ConfidentialInstructionBuildFailed))?;

    invoke(
        &ix,
        &[
            ctx.accounts.token_account.to_account_info(),
            ctx.accounts.authority.to_account_info(),
            ctx.accounts.token_program.to_account_info(),
        ],
    )?;

    msg!(
        "applied pending balance for account {}",
        ctx.accounts.token_account.key()
    );
    Ok(())
}


/// Task 6, step 4: a confidential Transfer.
///
/// Needs three proofs, each pre-verified into its own context state
/// account: an equality proof (the sender's new balance is correctly
/// computed), a ciphertext validity proof (the transfer amount is
/// well-formed under both sender's and recipient's ElGamal keys), and a
/// range proof (the amounts involved are non-negative and in range).
/// This instruction only references those three accounts — it does not
/// generate or inspect any proof itself.
#[derive(Accounts)]
pub struct ConfidentialTransfer<'info> {
    pub authority: Signer<'info>,

    /// CHECK: validated by Token-2022.
    #[account(mut, owner = token_program.key())]
    pub source: UncheckedAccount<'info>,

    /// CHECK: validated by Token-2022.
    #[account(owner = token_program.key())]
    pub mint: UncheckedAccount<'info>,

    /// CHECK: validated by Token-2022.
    #[account(mut, owner = token_program.key())]
    pub destination: UncheckedAccount<'info>,

    /// CHECK: a context state account holding a verified equality proof.
    pub equality_proof_context: UncheckedAccount<'info>,

    /// CHECK: a context state account holding a verified ciphertext
    /// validity proof.
    pub ciphertext_validity_proof_context: UncheckedAccount<'info>,

    /// CHECK: a context state account holding a verified range proof.
    pub range_proof_context: UncheckedAccount<'info>,

    pub token_program: Interface<'info, TokenInterface>,
}

use anchor_spl::token_interface::spl_token_2022::solana_zk_sdk::encryption::pod::elgamal::PodElGamalCiphertext;

pub fn handle_confidential_transfer(
    ctx: Context<ConfidentialTransfer>,
    new_source_decryptable_available_balance: [u8; AE_CIPHERTEXT_LEN],
    source_decrypt_handle: [u8; 64],
    destination_decrypt_handle: [u8; 64],
) -> Result<()> {
    let equality_key = ctx.accounts.equality_proof_context.key();
    let validity_key = ctx.accounts.ciphertext_validity_proof_context.key();
    let range_key = ctx.accounts.range_proof_context.key();

    let balance = DecryptableBalance::from(new_source_decryptable_available_balance);
    let source_handle: PodElGamalCiphertext = bytemuck::cast(source_decrypt_handle);
    let destination_handle: PodElGamalCiphertext = bytemuck::cast(destination_decrypt_handle);

    let ixs = confidential_instruction::transfer(
        &ctx.accounts.token_program.key(),
        &ctx.accounts.source.key(),
        &ctx.accounts.mint.key(),
        &ctx.accounts.destination.key(),
        &balance,
        &source_handle,
        &destination_handle,
        &ctx.accounts.authority.key(),
        &[],
        ProofLocation::ContextStateAccount(&range_key),
        ProofLocation::ContextStateAccount(&equality_key),
        ProofLocation::ContextStateAccount(&validity_key),
    )
    .map_err(|_| error!(ErrorCode::ConfidentialInstructionBuildFailed))?;

    let infos = [
        ctx.accounts.source.to_account_info(),
        ctx.accounts.mint.to_account_info(),
        ctx.accounts.destination.to_account_info(),
        ctx.accounts.authority.to_account_info(),
        ctx.accounts.equality_proof_context.to_account_info(),
        ctx.accounts.ciphertext_validity_proof_context.to_account_info(),
        ctx.accounts.range_proof_context.to_account_info(),
        ctx.accounts.token_program.to_account_info(),
    ];

    for ix in ixs {
        invoke(&ix, &infos)?;
    }

    msg!(
        "confidential transfer from {} to {}",
        ctx.accounts.source.key(),
        ctx.accounts.destination.key()
    );
    Ok(())
}

/// Task 6, step 5: WithdrawConfidentialTokens.
///
/// Needs two proofs: an equality proof (the remaining available balance
/// after withdrawal matches its commitment) and a range proof (that
/// remaining balance is non-negative). Per the task, pending balance must
/// already be applied before this runs — this instruction does not apply
/// it itself, since that's a distinct, separately-authorized step (step
/// 3) with its own ciphertext the owner must supply.
#[derive(Accounts)]
pub struct WithdrawConfidentialTokens<'info> {
    pub authority: Signer<'info>,

    /// CHECK: validated by Token-2022.
    #[account(mut, owner = token_program.key())]
    pub token_account: UncheckedAccount<'info>,

    /// CHECK: validated by Token-2022.
    #[account(owner = token_program.key())]
    pub mint: UncheckedAccount<'info>,

    /// CHECK: a context state account holding a verified equality proof.
    pub equality_proof_context: UncheckedAccount<'info>,

    /// CHECK: a context state account holding a verified range proof.
    pub range_proof_context: UncheckedAccount<'info>,

    pub token_program: Interface<'info, TokenInterface>,
}

pub fn handle_withdraw_confidential_tokens(
    ctx: Context<WithdrawConfidentialTokens>,
    amount: u64,
    decimals: u8,
    new_decryptable_available_balance: [u8; AE_CIPHERTEXT_LEN],
) -> Result<()> {
    let equality_key = ctx.accounts.equality_proof_context.key();
    let range_key = ctx.accounts.range_proof_context.key();

    let balance = DecryptableBalance::from(new_decryptable_available_balance);

    let ixs = confidential_instruction::withdraw(
        &ctx.accounts.token_program.key(),
        &ctx.accounts.token_account.key(),
        &ctx.accounts.mint.key(),
        amount,
        decimals,
        &balance,
        &ctx.accounts.authority.key(),
        &[],
        ProofLocation::ContextStateAccount(&equality_key),
        ProofLocation::ContextStateAccount(&range_key),
    )
    .map_err(|_| error!(ErrorCode::ConfidentialInstructionBuildFailed))?;

    let infos = [
        ctx.accounts.token_account.to_account_info(),
        ctx.accounts.mint.to_account_info(),
        ctx.accounts.authority.to_account_info(),
        ctx.accounts.equality_proof_context.to_account_info(),
        ctx.accounts.range_proof_context.to_account_info(),
        ctx.accounts.token_program.to_account_info(),
    ];

    for ix in ixs {
        invoke(&ix, &infos)?;
    }

    msg!(
        "withdrew {} (decimals {}) from confidential balance of {}",
        amount,
        decimals,
        ctx.accounts.token_account.key()
    );
    Ok(())
}