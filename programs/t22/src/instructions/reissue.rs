use anchor_lang::prelude::*;
use anchor_lang::solana_program::program::invoke;
use anchor_spl::token_2022_extensions::permanent_delegate::{
    permanent_delegate_initialize, PermanentDelegateInitialize,
};
use anchor_spl::token_interface::spl_token_2022::extension::confidential_transfer_fee::instruction as confidential_fee_instruction;
use anchor_spl::token_interface::{
    initialize_mint2, mint_close_authority_initialize, transfer_fee_initialize,
    InitializeMint2, MintCloseAuthorityInitialize, TokenInterface, TransferFeeInitialize,
};
use anchor_spl::token_interface::spl_token_2022::{
    extension::{
        confidential_transfer::instruction as confidential_instruction,
        default_account_state::instruction::initialize_default_account_state,
        metadata_pointer::instruction::initialize as initialize_metadata_pointer,
        ExtensionType,
    },
    state::{AccountState, Mint as MintState},
};

/// Task 5 + the scenario: re-issue the mint.
///
/// Confidential transfers cannot be added to a mint after creation — the
/// ConfidentialTransferMint extension, like every mint extension, must be
/// present before InitializeMint2 runs, and InitializeMint2 already ran on
/// the Task 1 mint. There is no "upgrade" path. The only option is a new
/// mint, so this instruction re-creates the full extension set from Task 1
/// and adds three new extensions on top of it:
///
///   - PermanentDelegate: the seizure authority regulators require.
///   - ConfidentialTransferMint, with auto_approve_new_accounts = false.
///     That flag is the "approve_policy = manual" the task asks for.
///   - ConfidentialTransferFeeConfig — required, not optional, once
///     TransferFeeConfig and ConfidentialTransferMint are both present.
///     Token-2022 will reject InitializeMint2 with
///     InvalidExtensionCombination otherwise: a plaintext transfer fee
///     can't be computed on an amount nobody can see, so the fee itself
///     has to become an encrypted quantity too, withheld under this
///     authority's ElGamal key. This is the gap the scenario points at —
///     "confidentiality" and "a fee on every transfer" don't compose for
///     free, they require a third piece to reconcile them.
///
/// The gap this re-issuance does NOT close: everything that happened on
/// the *old* mint — every public balance, every past transfer — stays
/// exactly as visible and seizable as it always was. Re-issuing only
/// changes what's possible going forward, on the new mint. See the
/// written finding for what this gap means for a sanctioned holder who
/// moves first.
#[derive(Accounts)]
pub struct ReissueMint<'info> {
    #[account(mut)]
    pub payer: Signer<'info>,

    /// CHECK: created and initialized in the handler.
    #[account(mut, signer)]
    pub mint: UncheckedAccount<'info>,

    pub token_program: Interface<'info, TokenInterface>,
    pub system_program: Program<'info, System>,
}

pub fn handle_reissue_mint(
    ctx: Context<ReissueMint>,
    decimals: u8,
    transfer_fee_basis_points: u16,
    maximum_fee: u64,
    withdraw_withheld_authority_elgamal_pubkey: [u8; 32],
) -> Result<()> {
    let extensions = [
        ExtensionType::MetadataPointer,
        ExtensionType::TransferFeeConfig,
        ExtensionType::DefaultAccountState,
        ExtensionType::MintCloseAuthority,
        ExtensionType::PermanentDelegate,
        ExtensionType::ConfidentialTransferMint,
        ExtensionType::ConfidentialTransferFeeConfig,
    ];

    let space = ExtensionType::try_calculate_account_len::<MintState>(&extensions)?;
    let lamports = Rent::get()?.minimum_balance(space);

    anchor_lang::system_program::create_account(
        CpiContext::new(
            ctx.accounts.system_program.key(),
            anchor_lang::system_program::CreateAccount {
                from: ctx.accounts.payer.to_account_info(),
                to: ctx.accounts.mint.to_account_info(),
            },
        ),
        lamports,
        space as u64,
        &ctx.accounts.token_program.key(),
    )?;

    let mint_info = ctx.accounts.mint.to_account_info();
    let program_info = ctx.accounts.token_program.to_account_info();
    let infos = [mint_info.clone(), program_info.clone()];

    // The original four, carried forward unchanged.
    invoke(
        &initialize_metadata_pointer(
            &ctx.accounts.token_program.key(),
            &ctx.accounts.mint.key(),
            Some(ctx.accounts.payer.key()),
            Some(ctx.accounts.mint.key()),
        )?,
        &infos,
    )?;

    transfer_fee_initialize(
        CpiContext::new(
            ctx.accounts.token_program.key(),
            TransferFeeInitialize {
                token_program_id: program_info.clone(),
                mint: mint_info.clone(),
            },
        ),
        Some(&ctx.accounts.payer.key()),
        Some(&ctx.accounts.payer.key()),
        transfer_fee_basis_points,
        maximum_fee,
    )?;

    invoke(
        &initialize_default_account_state(
            &ctx.accounts.token_program.key(),
            &ctx.accounts.mint.key(),
            &AccountState::Frozen,
        )?,
        &infos,
    )?;

    mint_close_authority_initialize(
        CpiContext::new(
            ctx.accounts.token_program.key(),
            MintCloseAuthorityInitialize {
                token_program_id: program_info.clone(),
                mint: mint_info.clone(),
            },
        ),
        Some(&ctx.accounts.payer.key()),
    )?;

    // New: seizure authority.
    permanent_delegate_initialize(
        CpiContext::new(
            ctx.accounts.token_program.key(),
            PermanentDelegateInitialize {
                token_program_id: program_info.clone(),
                mint: mint_info.clone(),
            },
        ),
        &ctx.accounts.payer.key(),
    )?;

    // New: confidential transfers, manual approval.
    invoke(
        &confidential_instruction::initialize_mint(
            &ctx.accounts.token_program.key(),
            &ctx.accounts.mint.key(),
            Some(ctx.accounts.payer.key()), // confidential transfer mint authority
            false,                          // auto_approve_new_accounts = manual
            None,                           // no auditor ElGamal key
        )?,
        &infos,
    )?;

    // Must come after ConfidentialTransferMint — Token-2022 checks the
    // confidential mint config already exists before allowing this.
    invoke(
        &confidential_fee_instruction::initialize_confidential_transfer_fee_config(
            &ctx.accounts.token_program.key(),
            &ctx.accounts.mint.key(),
            Some(ctx.accounts.payer.key()),
            &withdraw_withheld_authority_elgamal_pubkey.into(),
        )?,
        &infos,
    )?;

    initialize_mint2(
        CpiContext::new(
            ctx.accounts.token_program.key(),
            InitializeMint2 { mint: mint_info },
        ),
        decimals,
        &ctx.accounts.payer.key(),
        Some(&ctx.accounts.payer.key()),
    )?;

    msg!(
        "mint {} re-issued, {} bytes, 7 extensions (added PermanentDelegate + ConfidentialTransferMint + ConfidentialTransferFeeConfig)",
        ctx.accounts.mint.key(),
        space
    );
    Ok(())
}