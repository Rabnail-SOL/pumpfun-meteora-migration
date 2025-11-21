use anchor_lang::prelude::*;
use anchor_spl::{
    associated_token::AssociatedToken,
    token_interface::{self, Mint, TokenAccount, TokenInterface, TransferChecked},
};
use crate::states::{Global, BondingCurve};
use crate::errors::ErrorCode;

#[derive(Accounts)]
pub struct Withdraw<'info> {
    #[account(mut)]
    pub authority: Signer<'info>,
    #[account(
        seeds = [b"global"],
        bump,
        constraint = global.authority == authority.key()
    )]
    pub global: Account<'info, Global>,
    #[account(
        constraint = bonding_curve.mint == mint.key()
    )]
    pub mint: InterfaceAccount<'info, Mint>,
    #[account(
        mut,
        seeds = [b"bonding_curve", mint.key().as_ref()],
        bump,
    )]
    pub bonding_curve: Account<'info, BondingCurve>,
    #[account(
        mut,
        associated_token::mint = mint,
        associated_token::authority = bonding_curve
    )]
    pub bonding_curve_ata: InterfaceAccount<'info, TokenAccount>,
    #[account(
        mut,
        associated_token::mint = mint,
        associated_token::authority = authority
    )]
    pub authority_ata: InterfaceAccount<'info, TokenAccount>,
    pub system_program: Program<'info, System>,
    pub token_program: Interface<'info, TokenInterface>,
    pub associated_token_program: Program<'info, AssociatedToken>,
    pub rent: Sysvar<'info, Rent>,
}

pub fn handler(ctx: Context<Withdraw>) -> Result<()> {
    let curve = &mut ctx.accounts.bonding_curve;
    require!(curve.complete, ErrorCode::BondingCurveNotComplete);

    // Withdraw all tokens from the bonding curve's ATA
    let token_balance = ctx.accounts.bonding_curve_ata.amount;
    if token_balance > 0 {
        let seeds: &[&[&[u8]]] = &[&[
            b"bonding_curve",
            ctx.accounts.mint.to_account_info().key.as_ref(),
            &[ctx.bumps.bonding_curve],
        ]];
        let decimals = ctx.accounts.mint.decimals;
        let cpi_accounts = TransferChecked {
            mint: ctx.accounts.mint.to_account_info(),
            from: ctx.accounts.bonding_curve_ata.to_account_info(),
            to: ctx.accounts.authority_ata.to_account_info(),
            authority: curve.to_account_info(),
        };
        let cpi_program = ctx.accounts.token_program.to_account_info();
        let cpi_context = CpiContext::new(cpi_program, cpi_accounts).with_signer(seeds);
        token_interface::transfer_checked(cpi_context, token_balance, decimals)?;
    }

    // Withdraw all SOL from the bonding curve account, leaving exact rent.
    let rent = Rent::get()?;
    let rent_exempt_minimum = rent.minimum_balance(curve.to_account_info().data_len());
    let current_balance = curve.to_account_info().lamports();

    if current_balance > rent_exempt_minimum {
        let withdrawable_sol = current_balance
            .checked_sub(rent_exempt_minimum)
            .ok_or(ProgramError::ArithmeticOverflow)?;
        curve.sub_lamports(withdrawable_sol)?;
        ctx.accounts.authority.add_lamports(withdrawable_sol)?;
    }

    Ok(())
}
