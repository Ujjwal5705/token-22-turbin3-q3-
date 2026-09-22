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
}