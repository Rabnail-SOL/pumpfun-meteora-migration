use anchor_lang::prelude::*;
use anchor_spl::{
    associated_token::AssociatedToken,
    token_interface::{self, Mint, TokenAccount, TokenInterface, TransferChecked},
};
use crate::states::{Global, BondingCurve};
use crate::errors::ErrorCode;
use crate::events::{Trade, TradeSide};
use crate::consts::BPS_DENOMINATOR;

#[derive(Accounts)]
pub struct Sell<'info> {
    #[account(mut)]
    pub signer: Signer<'info>,
    #[account(
        mut,
        seeds = [b"bonding_curve", mint.key().as_ref()],
        bump
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
        associated_token::authority = signer
    )]
    pub user_ata: InterfaceAccount<'info, TokenAccount>,
    #[account(
        constraint = bonding_curve.mint == mint.key()
    )]
    pub mint: InterfaceAccount<'info, Mint>,
    #[account(
        seeds = [b"global"],
        bump
    )]
    pub global: Account<'info, Global>,
    #[account(mut, constraint = global.platform_fee_recipient == platform_fee_recipient.key())]
    pub platform_fee_recipient: SystemAccount<'info>,
    /// CHECK: Global reserve PDA (authority for all reserve ATAs)
    #[account(
        seeds = [b"reserve"], bump,
        constraint = global.reserve == global_reserve.key()
    )]
    pub global_reserve: UncheckedAccount<'info>,
    #[account(
        init_if_needed,
        payer = signer,
        associated_token::mint = mint,
        associated_token::authority = global_reserve,
        associated_token::token_program = token_program,
    )]
    pub reserve_ata: InterfaceAccount<'info, TokenAccount>,
    pub system_program: Program<'info, System>,
    pub token_program: Interface<'info, TokenInterface>,
    pub associated_token_program: Program<'info, AssociatedToken>,
}

pub fn handler(ctx: Context<Sell>, token_amount: u64, min_sol_output: u64) -> Result<()> {
    let curve = &mut ctx.accounts.bonding_curve;
    require!(!curve.complete, ErrorCode::BondingCurveComplete);
    require_gt!(token_amount, 0);

    // Calculate SOL output
    let k = u128::from(curve.virtual_sol_reserves)
        .checked_mul(u128::from(curve.virtual_token_reserves))
        .ok_or(ProgramError::ArithmeticOverflow)?;
    let new_virtual_token_reserves = curve
        .virtual_token_reserves
        .checked_add(token_amount)
        .ok_or(ProgramError::ArithmeticOverflow)?;
    let new_virtual_sol_reserves = k
        .checked_div(u128::from(new_virtual_token_reserves))
        .ok_or(ProgramError::ArithmeticOverflow)?;
    let sol_out_gross = curve
        .virtual_sol_reserves
        .checked_sub(new_virtual_sol_reserves as u64)
        .ok_or(ProgramError::ArithmeticOverflow)?;

    // Calculate fees (split between platform and reserve)
    let total_fee_bps = ctx
        .accounts
        .global
        .platform_trade_fee_bps
        .checked_add(ctx.accounts.global.reserve_trade_fee_bps)
        .ok_or(ProgramError::ArithmeticOverflow)?;
    let fee = sol_out_gross
        .checked_mul(total_fee_bps)
        .and_then(|res| res.checked_div(BPS_DENOMINATOR))
        .ok_or(ProgramError::ArithmeticOverflow)?;

    let platform_fee = sol_out_gross
        .checked_mul(ctx.accounts.global.platform_trade_fee_bps)
        .and_then(|res| res.checked_div(BPS_DENOMINATOR))
        .ok_or(ProgramError::ArithmeticOverflow)?;
    let reserve_fee = sol_out_gross
        .checked_mul(ctx.accounts.global.reserve_trade_fee_bps)
        .and_then(|res| res.checked_div(BPS_DENOMINATOR))
        .ok_or(ProgramError::ArithmeticOverflow)?;

    let sol_out_net = sol_out_gross
        .checked_sub(fee)
        .ok_or(ProgramError::ArithmeticOverflow)?;

    // Calculate tokens for reserve if reserve_fee > 0
    // Reserve fee is used to buy tokens back from the curve at the rate after user's sell
    let mut reserve_tokens_out: u64 = 0;
    if reserve_fee > 0 {
        // Use reserve fee to buy tokens at the current rate (after user's sell)
        // new_virtual_sol_reserves already accounts for the user's sell
        // Reserve fee buys tokens, so we add it to virtual SOL reserves
        let reserve_k = u128::from(new_virtual_sol_reserves as u64)
            .checked_mul(u128::from(new_virtual_token_reserves))
            .ok_or(ProgramError::ArithmeticOverflow)?;
        let reserve_new_virtual_sol_reserves = (new_virtual_sol_reserves as u64)
            .checked_add(reserve_fee)
            .ok_or(ProgramError::ArithmeticOverflow)?;
        let reserve_new_virtual_token_reserves = reserve_k
            .checked_div(u128::from(reserve_new_virtual_sol_reserves))
            .ok_or(ProgramError::ArithmeticOverflow)?;
        reserve_tokens_out = new_virtual_token_reserves
            .checked_sub(reserve_new_virtual_token_reserves as u64)
            .ok_or(ProgramError::ArithmeticOverflow)?;
        
        // Update virtual reserves for reserve purchase
        curve.virtual_sol_reserves = reserve_new_virtual_sol_reserves;
        curve.virtual_token_reserves = reserve_new_virtual_token_reserves as u64;
    } else {
        // No reserve fee, just update for user's sell
        curve.virtual_sol_reserves = new_virtual_sol_reserves as u64;
        curve.virtual_token_reserves = new_virtual_token_reserves;
    }

    // Validation
    require_gte!(sol_out_net, min_sol_output);
    
    // State Updates for user's sell
    // User receives net SOL (gross - all fees)
    // Platform fee goes out, reserve fee stays in curve (used to buy tokens)
    // So real SOL reserves decrease by: sol_out_gross - reserve_fee
    let sol_removed_from_curve = sol_out_gross
        .checked_sub(reserve_fee)
        .ok_or(ProgramError::ArithmeticOverflow)?;
    curve.real_sol_reserves = curve
        .real_sol_reserves
        .checked_sub(sol_removed_from_curve)
        .ok_or(ProgramError::ArithmeticOverflow)?;
    // User's tokens are added back
    curve.real_token_reserves = curve
        .real_token_reserves
        .checked_add(token_amount)
        .ok_or(ProgramError::ArithmeticOverflow)?;
    
    // Reserve tokens are deducted (if reserve_fee > 0)
    if reserve_fee > 0 {
        curve.real_token_reserves = curve
            .real_token_reserves
            .checked_sub(reserve_tokens_out)
            .ok_or(ProgramError::ArithmeticOverflow)?;
    }

    // Token Transfer (CPI) - User sends tokens to bonding curve
    let decimals = ctx.accounts.mint.decimals;
    let cpi_accounts = TransferChecked {
        mint: ctx.accounts.mint.to_account_info(),
        from: ctx.accounts.user_ata.to_account_info(),
        to: ctx.accounts.bonding_curve_ata.to_account_info(),
        authority: ctx.accounts.signer.to_account_info(),
    };
    let cpi_program = ctx.accounts.token_program.to_account_info();
    let cpi_context = CpiContext::new(cpi_program, cpi_accounts);
    token_interface::transfer_checked(cpi_context, token_amount, decimals)?;

    // Reserve fee is used to buy tokens back from the curve
    // Transfer reserve tokens to reserve ATA
    if reserve_fee > 0 {
        let mint_key = ctx.accounts.mint.key();
        let bonding_curve_seeds: &[&[&[u8]]] = &[&[
            b"bonding_curve",
            mint_key.as_ref(),
            &[ctx.bumps.bonding_curve],
        ]];
        let reserve_cpi_accounts = TransferChecked {
            mint: ctx.accounts.mint.to_account_info(),
            from: ctx.accounts.bonding_curve_ata.to_account_info(),
            to: ctx.accounts.reserve_ata.to_account_info(),
            authority: ctx.accounts.bonding_curve.to_account_info(),
        };
        let reserve_cpi_program = ctx.accounts.token_program.to_account_info();
        let reserve_cpi_context = CpiContext::new(reserve_cpi_program, reserve_cpi_accounts)
            .with_signer(bonding_curve_seeds);
        token_interface::transfer_checked(reserve_cpi_context, reserve_tokens_out, decimals)?;
    }

    // SOL Transfers using direct lamport manipulation (PDA cannot use CPI to send SOL)
    // Total lamports must balance: curve loses (platform_fee + sol_out_net) = sol_removed_from_curve
    ctx.accounts.bonding_curve.sub_lamports(platform_fee.checked_add(sol_out_net).unwrap())?;
    
    if platform_fee > 0 {
        ctx.accounts.platform_fee_recipient.add_lamports(platform_fee)?;
    }
    
    ctx.accounts.signer.add_lamports(sol_out_net)?;

    // Emit trade event
    emit!(Trade {
        mint: ctx.accounts.mint.key(),
        trader: ctx.accounts.signer.key(),
        side: TradeSide::Sell,
        sol_amount: sol_out_net,
        token_amount,
    });

    Ok(())
}
