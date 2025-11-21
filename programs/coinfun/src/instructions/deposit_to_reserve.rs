use anchor_lang::prelude::*;
use anchor_spl::token_interface::{self, Mint, TokenAccount, TokenInterface, TransferChecked};
use crate::states::{Global, BondingCurve};
use crate::errors::ErrorCode;

#[derive(Accounts)]
pub struct DepositToReserve<'info> {
    #[account(mut)]
    pub authority: Signer<'info>,
    #[account(
        seeds = [b"global"],
        bump,
        constraint = global.authority == authority.key()
    )]
    pub global: Account<'info, Global>,
    /// CHECK: Global reserve PDA (authority for all reserve ATAs)
    #[account(
        seeds = [b"reserve"], bump,
        constraint = global.reserve == global_reserve.key()
    )]
    pub global_reserve: UncheckedAccount<'info>,
    #[account(
        constraint = bonding_curve.mint == mint.key()
    )]
    pub mint: InterfaceAccount<'info, Mint>,
    #[account(
        seeds = [b"bonding_curve", mint.key().as_ref()],
        bump,
        constraint = bonding_curve.mint == mint.key()
    )]
    pub bonding_curve: Account<'info, BondingCurve>,
    #[account(
        mut,
        associated_token::mint = mint,
        associated_token::authority = global_reserve
    )]
    pub reserve_ata: InterfaceAccount<'info, TokenAccount>,
    #[account(
        mut,
        associated_token::mint = mint,
        associated_token::authority = authority
    )]
    pub authority_ata: InterfaceAccount<'info, TokenAccount>,
    pub token_program: Interface<'info, TokenInterface>,
}

pub fn handler(ctx: Context<DepositToReserve>, amount: u64) -> Result<()> {
    require!(amount > 0, ErrorCode::NothingToWithdraw);

    let decimals = ctx.accounts.mint.decimals;
    // Transfer tokens from authority's ATA to the reserve ATA
    let cpi_accounts = TransferChecked {
        mint: ctx.accounts.mint.to_account_info(),
        from: ctx.accounts.authority_ata.to_account_info(),
        to: ctx.accounts.reserve_ata.to_account_info(),
        authority: ctx.accounts.authority.to_account_info(),
    };
    let cpi_program = ctx.accounts.token_program.to_account_info();
    let cpi_context = CpiContext::new(cpi_program, cpi_accounts);
    token_interface::transfer_checked(cpi_context, amount, decimals)?;

    Ok(())
}
