use anchor_lang::prelude::*;
use anchor_lang::solana_program::program::invoke;
use anchor_spl::token_interface::{
    initialize_mint2, mint_close_authority_initialize, transfer_fee_initialize,
    InitializeMint2, MintCloseAuthorityInitialize, TokenInterface, TransferFeeInitialize,
};
use anchor_spl::token_interface::spl_token_2022::{
    extension::{
        default_account_state::instruction::initialize_default_account_state,
        metadata_pointer::instruction::initialize as initialize_metadata_pointer,
        ExtensionType,
    },
    state::{AccountState, Mint as MintState},
};

#[derive(Accounts)]
pub struct InitializeMint<'info> {
    #[account(mut)]
    pub payer: Signer<'info>,

    /// CHECK: created and initialized in the handler; must sign because the
    /// account is created at its own address.
    #[account(mut, signer)]
    pub mint: UncheckedAccount<'info>,

    pub token_program: Interface<'info, TokenInterface>,
    pub system_program: Program<'info, System>,
}

pub fn handle_initialize_mint(
    ctx: Context<InitializeMint>,
    decimals: u8,
    transfer_fee_basis_points: u16,
    maximum_fee: u64,
) -> Result<()> {
    let extensions = [
        ExtensionType::MetadataPointer,
        ExtensionType::TransferFeeConfig,
        ExtensionType::DefaultAccountState,
        ExtensionType::MintCloseAuthority,
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
        "stablecoin mint {} created, {} bytes, {} bps fee, frozen by default",
        ctx.accounts.mint.key(),
        space,
        transfer_fee_basis_points
    );
    Ok(())
}