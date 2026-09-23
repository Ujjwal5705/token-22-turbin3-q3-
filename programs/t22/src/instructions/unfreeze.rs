use anchor_lang::prelude::*;
use anchor_spl::token_interface::{thaw_account, ThawAccount, TokenInterface};

/// Task 4: unfreeze a single account after KYC clears.
///
/// This is deliberately narrow. It thaws exactly the one token account
/// passed in, using the mint's freeze authority as the signer. It does not
/// touch the mint's DefaultAccountState extension at all — that extension
/// stays set to Frozen permanently, so every *new* account for this mint
/// is still born frozen and still requires its own KYC clearance and its
/// own call to this instruction. There is no mint-level switch that
/// unfreezes accounts in bulk; each thaw is a distinct, individually
/// authorized action, which is the point of gating onboarding by KYC in
/// the first place.
#[derive(Accounts)]
pub struct UnfreezeAccount<'info> {
    /// The mint's freeze authority. Token-2022 checks this against the
    /// authority recorded on the mint; anchor-spl's `thaw_account` CPI
    /// does not further constrain who this is, so the check is entirely
    /// enforced token-program-side.
    pub freeze_authority: Signer<'info>,

    /// CHECK: validated by Token-2022 during thaw_account — must be a
    /// token account for `mint`, and must currently be frozen.
    #[account(mut, owner = token_program.key())]
    pub token_account: UncheckedAccount<'info>,

    /// CHECK: validated by Token-2022; must be the mint whose freeze
    /// authority matches the signer above.
    #[account(owner = token_program.key())]
    pub mint: UncheckedAccount<'info>,

    pub token_program: Interface<'info, TokenInterface>,
}

pub fn handle_unfreeze_account(ctx: Context<UnfreezeAccount>) -> Result<()> {
    thaw_account(CpiContext::new(
        ctx.accounts.token_program.key(),
        ThawAccount {
            account: ctx.accounts.token_account.to_account_info(),
            mint: ctx.accounts.mint.to_account_info(),
            authority: ctx.accounts.freeze_authority.to_account_info(),
        },
    ))?;

    msg!(
        "account {} thawed by freeze authority {} (KYC cleared) — mint default state unchanged",
        ctx.accounts.token_account.key(),
        ctx.accounts.freeze_authority.key()
    );
    Ok(())
}