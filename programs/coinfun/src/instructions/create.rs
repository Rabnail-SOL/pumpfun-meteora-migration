use anchor_lang::prelude::*;
use anchor_spl::{
    associated_token::AssociatedToken,
    metadata::{
        create_metadata_accounts_v3, mpl_token_metadata::types::DataV2, CreateMetadataAccountsV3,
        Metadata,
    },
    token_interface::{self, Mint, MintTo, TokenAccount, TokenInterface},
};
use crate::states::{Global, BondingCurve};
use crate::events::TokenCreated;

#[derive(Accounts)]
pub struct Create<'info> {
    #[account(mut)]
    pub signer: Signer<'info>,
    /// CHECK: The creator's address is used as a seed for the mint PDA.
    pub creator: UncheckedAccount<'info>,
    #[account(
        init,
        payer = signer,
        mint::decimals = 6,
        mint::authority = bonding_curve.key(),
    )]
    pub mint: InterfaceAccount<'info, Mint>,

    #[account(
        seeds = [b"global"],
        bump
    )]
    pub global: Account<'info, Global>,
    #[account(
        init,
        payer = signer,
        space = 8 + BondingCurve::INIT_SPACE,
        seeds = [b"bonding_curve",mint.key().as_ref()], bump
    )]
    pub bonding_curve: Account<'info, BondingCurve>,
    #[account(
        init,
        payer = signer,
        associated_token::mint = mint,
        associated_token::authority = bonding_curve,
        associated_token::token_program = token_program,
    )]
    pub bonding_curve_ata: InterfaceAccount<'info, TokenAccount>,
    /// CHECK: Validate address by deriving pda
    #[account(
        mut,
        seeds = [b"metadata", token_metadata_program.key().as_ref(), mint.key().as_ref()],
        bump,
        seeds::program = token_metadata_program.key(),
    )]
    pub metadata_account: UncheckedAccount<'info>,
    pub token_metadata_program: Program<'info, Metadata>,
    pub associated_token_program: Program<'info, AssociatedToken>,
    pub token_program: Interface<'info, TokenInterface>,
    pub system_program: Program<'info, System>,
    pub rent: Sysvar<'info, Rent>,
}

pub fn handler(
    ctx: Context<Create>,
    token_name: String,
    token_symbol: String,
    token_uri: String,
) -> Result<()> {
    msg!("Creating metadata account...");
    msg!(
        "Metadata account address: {}",
        &ctx.accounts.metadata_account.key()
    );

    // mint initial_real_token_reserves to bonding_curve_ata
    let signer_seeds: &[&[&[u8]]] = &[&[
        b"bonding_curve",
        ctx.accounts.mint.to_account_info().key.as_ref(),
        &[ctx.bumps.bonding_curve],
    ]];
    // Cross Program Invocation (CPI)
    // Invoking the create_metadata_account_v3 instruction on the token metadata program
    create_metadata_accounts_v3(
        CpiContext::new(
            ctx.accounts.token_metadata_program.to_account_info(),
            CreateMetadataAccountsV3 {
                metadata: ctx.accounts.metadata_account.to_account_info(),
                mint: ctx.accounts.mint.to_account_info(),
                mint_authority: ctx.accounts.bonding_curve.to_account_info(),
                update_authority: ctx.accounts.creator.to_account_info(),
                payer: ctx.accounts.signer.to_account_info(),
                system_program: ctx.accounts.system_program.to_account_info(),
                rent: ctx.accounts.rent.to_account_info(),
            },
        )
        .with_signer(signer_seeds),
        DataV2 {
            name: token_name,
            symbol: token_symbol,
            uri: token_uri,
            seller_fee_basis_points: 0,
            creators: None,
            collection: None,
            uses: None,
        },
        false, // Is mutable
        false, // Update authority is creator not signer
        None,  // Collection details
    )?;

    msg!("Initializing bonding_curve");
    // initialize bonding_curve
    ctx.accounts.bonding_curve.set_inner(BondingCurve {
        mint: ctx.accounts.mint.key(),
        creator: ctx.accounts.creator.key(),
        virtual_token_reserves: ctx.accounts.global.initial_virtual_token_reserves,
        virtual_sol_reserves: ctx.accounts.global.initial_virtual_sol_reserves,
        real_token_reserves: ctx.accounts.global.token_total_supply,
        real_sol_reserves: 0,
        token_total_supply: ctx.accounts.global.token_total_supply,
        complete: false,
    });

    let cpi_accounts = MintTo {
        mint: ctx.accounts.mint.to_account_info(),
        to: ctx.accounts.bonding_curve_ata.to_account_info(),
        authority: ctx.accounts.bonding_curve.to_account_info(),
    };
    let cpi_program = ctx.accounts.token_program.to_account_info();
    let cpi_context = CpiContext::new(cpi_program, cpi_accounts).with_signer(signer_seeds);
    token_interface::mint_to(cpi_context, ctx.accounts.global.token_total_supply)?;

    // Emit event
    emit!(TokenCreated {
        mint: ctx.accounts.mint.key(),
        creator: ctx.accounts.creator.key(),
    });

    Ok(())
}
