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
}