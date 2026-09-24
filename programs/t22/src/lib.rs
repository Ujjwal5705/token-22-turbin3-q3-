use anchor_lang::prelude::*;

mod constants;
mod error;
mod instructions;
mod state;

use instructions::*;

declare_id!("JBkN5Y4D3TqaR35s9tQypwLvc9Wq9Prsk9QgKeY9b2BV");

#[program]
pub mod t22 {
    use super::*;

    /// Task 1: create the remittance stablecoin mint — MetadataPointer,
    /// TransferFeeConfig, DefaultAccountState(Frozen) and
    /// MintCloseAuthority, all stacked on one mint.
    pub fn initialize_mint(
        ctx: Context<InitializeMint>,
        decimals: u8,
        transfer_fee_basis_points: u16,
        maximum_fee: u64,
    ) -> Result<()> {
        handle_initialize_mint(ctx, decimals, transfer_fee_basis_points, maximum_fee)
    }

    /// Task 2: transfer using transfer_checked_with_fee, with the fee
    /// computed live from the mint's current-epoch TransferFeeConfig
    /// rather than trusted from the caller or a cached rate.
    pub fn transfer_with_fee(
        ctx: Context<TransferWithFee>,
        amount: u64,
        decimals: u8,
    ) -> Result<()> {
        handle_transfer_with_fee(ctx, amount, decimals)
    }

    /// Task 4: thaw one account after KYC clears. Separate from any
    /// mint-level DefaultAccountState change — new accounts are still
    /// born frozen after this call.
    pub fn unfreeze_account(ctx: Context<UnfreezeAccount>) -> Result<()> {
        handle_unfreeze_account(ctx)
    }

    /// Task 5: re-issue the mint. Confidential transfers cannot be added
    /// after creation, so this is a fresh mint carrying forward Task 1's
    /// four extensions plus PermanentDelegate (seizure authority) and
    /// ConfidentialTransferMint with manual account approval.
    pub fn reissue_mint(
        ctx: Context<ReissueMint>,
        decimals: u8,
        transfer_fee_basis_points: u16,
        maximum_fee: u64,
        withdraw_withheld_authority_elgamal_pubkey: [u8; 32],
    ) -> Result<()> {
        handle_reissue_mint(
            ctx,
            decimals,
            transfer_fee_basis_points,
            maximum_fee,
            withdraw_withheld_authority_elgamal_pubkey,
        )
    }

    /// Task 6, step 1: owner-only confidential-transfer opt-in for one
    /// account, separate from ATA creation.
    pub fn configure_confidential_account(
        ctx: Context<ConfigureConfidentialAccount>,
        decryptable_zero_balance: [u8; AE_CIPHERTEXT_LEN],
        maximum_pending_balance_credit_counter: u64,
    ) -> Result<()> {
        handle_configure_confidential_account(
            ctx,
            decryptable_zero_balance,
            maximum_pending_balance_credit_counter,
        )
    }

    /// Task 6, step 2: move tokens into the pending confidential balance.
    pub fn deposit_confidential_tokens(
        ctx: Context<DepositConfidentialTokens>,
        amount: u64,
        decimals: u8,
    ) -> Result<()> {
        handle_deposit_confidential_tokens(ctx, amount, decimals)
    }

    /// Task 6, step 3: move pending confidential balance into available.
    pub fn apply_confidential_pending_balance(
        ctx: Context<ApplyConfidentialPendingBalance>,
        expected_pending_balance_credit_counter: u64,
        new_decryptable_available_balance: [u8; AE_CIPHERTEXT_LEN],
    ) -> Result<()> {
        handle_apply_confidential_pending_balance(
            ctx,
            expected_pending_balance_credit_counter,
            new_decryptable_available_balance,
        )
    }

    /// Task 6, step 4: confidential transfer between two configured accounts.
    pub fn confidential_transfer(
        ctx: Context<ConfidentialTransfer>,
        new_source_decryptable_available_balance: [u8; AE_CIPHERTEXT_LEN],
        source_decrypt_handle: [u8; 64],
        destination_decrypt_handle: [u8; 64],
    ) -> Result<()> {
        handle_confidential_transfer(
            ctx,
            new_source_decryptable_available_balance,
            source_decrypt_handle,
            destination_decrypt_handle,
        )
    }

    /// Task 6, step 5: withdraw from confidential available balance back
    /// to the public balance. Apply pending balance (step 3) first.
    pub fn withdraw_confidential_tokens(
        ctx: Context<WithdrawConfidentialTokens>,
        amount: u64,
        decimals: u8,
        new_decryptable_available_balance: [u8; AE_CIPHERTEXT_LEN],
    ) -> Result<()> {
        handle_withdraw_confidential_tokens(
            ctx,
            amount,
            decimals,
            new_decryptable_available_balance,
        )
    }
}