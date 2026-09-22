use anchor_lang::prelude::*;
use anchor_spl::token_interface::{
    spl_token_2022, transfer_checked_with_fee, TokenInterface, TransferCheckedWithFee,
};
use spl_token_2022::extension::{
    transfer_fee::TransferFeeConfig, BaseStateWithExtensions, StateWithExtensions,
};
use spl_token_2022::state::Mint as MintState;

use crate::error::ErrorCode;

/// Task 2: transfer with a protocol-level fee.
///
/// Uses `transfer_checked_with_fee`, never plain `transfer` or
/// `transfer_checked` — those two do not enforce or even know about
/// TransferFeeConfig, so a transfer built with them can be front-run by a
/// fee-rate change between simulation and execution, or simply omit the
/// fee bookkeeping Token-2022 expects.
///
/// The fee itself is never cached or passed in by the caller. It is read
/// fresh from the mint's live TransferFeeConfig extension, for the
/// *current* epoch, via `calculate_epoch_fee`. Transfer fees are epoch
/// scheduled — a `newer_transfer_fee` can be queued to take effect at a
/// future epoch without being active yet — so asking for "the fee" without
/// specifying an epoch is not a well-formed question. Computing it here,
/// at transfer time, is the only way the fee actually charged can never
/// drift from what the mint currently enforces.
#[derive(Accounts)]
pub struct TransferWithFee<'info> {
    pub authority: Signer<'info>,

    /// CHECK: validated by Token-2022 during transfer_checked_with_fee.
    #[account(mut, owner = token_program.key())]
    pub source: UncheckedAccount<'info>,

    /// CHECK: read via StateWithExtensions in the handler; ownership
    /// checked so an account from another program can't be substituted.
    #[account(owner = token_program.key())]
    pub mint: UncheckedAccount<'info>,

    /// CHECK: validated by Token-2022 during transfer_checked_with_fee.
    #[account(mut, owner = token_program.key())]
    pub destination: UncheckedAccount<'info>,

    pub token_program: Interface<'info, TokenInterface>,
}

pub fn handle_transfer_with_fee(
    ctx: Context<TransferWithFee>,
    amount: u64,
    decimals: u8,
) -> Result<()> {
    // Read via StateWithExtensions only (Task 3) — never a raw/typed unpack.
    let fee_basis_points_and_max = {
        let mint_info = ctx.accounts.mint.to_account_info();
        let data = mint_info.try_borrow_data()?;
        let state = StateWithExtensions::<MintState>::unpack(&data)?;

        let config = state
            .get_extension::<TransferFeeConfig>()
            .map_err(|_| error!(ErrorCode::MissingTransferFeeConfig))?;

        let epoch = Clock::get()?.epoch;
        let fee = config.calculate_epoch_fee(epoch, amount);
        fee.ok_or(error!(ErrorCode::FeeCalculationOverflow))?
        // (data borrow and state both drop here, before the CPI below)
    };

    let fee = fee_basis_points_and_max;

    transfer_checked_with_fee(
        CpiContext::new(
            ctx.accounts.token_program.key(),
            TransferCheckedWithFee {
                token_program_id: ctx.accounts.token_program.to_account_info(),
                source: ctx.accounts.source.to_account_info(),
                mint: ctx.accounts.mint.to_account_info(),
                destination: ctx.accounts.destination.to_account_info(),
                authority: ctx.accounts.authority.to_account_info(),
            },
        ),
        amount,
        decimals,
        fee,
    )?;

    msg!(
        "transferred {} (decimals {}) from {} to {}, fee withheld {} (epoch-computed, not cached)",
        amount,
        decimals,
        ctx.accounts.source.key(),
        ctx.accounts.destination.key(),
        fee
    );
    Ok(())
}