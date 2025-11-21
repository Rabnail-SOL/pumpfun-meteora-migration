use anchor_lang::prelude::*;
use anchor_spl::{
    associated_token::AssociatedToken,
    token_interface::{self, Mint, TokenAccount, TokenInterface, TransferChecked},
};
use crate::states::{Global, BondingCurve};
use crate::errors::ErrorCode;
use crate::events::{Trade, TradeSide, CurveComplete};
use crate::consts::BPS_DENOMINATOR;

#[derive(Accounts)]
pub struct Buy<'info> {
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
        init_if_needed,
        payer = signer,
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

pub fn handler(ctx: Context<Buy>, sol_amount: u64, min_token_output: u64) -> Result<()> {
    let curve = &mut ctx.accounts.bonding_curve;
    require!(!curve.complete, ErrorCode::BondingCurveComplete);
    require_gt!(sol_amount, 0);

    // Calculate fees consistently for all trades
    let total_fee_bps = ctx
        .accounts
        .global
        .platform_trade_fee_bps
        .checked_add(ctx.accounts.global.reserve_trade_fee_bps)
        .ok_or(ProgramError::ArithmeticOverflow)?;
    let fee = sol_amount
        .checked_mul(total_fee_bps)
        .and_then(|res| res.checked_div(BPS_DENOMINATOR))
        .ok_or(ProgramError::ArithmeticOverflow)?;

    // Split fees between platform and reserve
    let platform_fee = sol_amount
        .checked_mul(ctx.accounts.global.platform_trade_fee_bps)
        .and_then(|res| res.checked_div(BPS_DENOMINATOR))
        .ok_or(ProgramError::ArithmeticOverflow)?;
    let reserve_fee = sol_amount
        .checked_mul(ctx.accounts.global.reserve_trade_fee_bps)
        .and_then(|res| res.checked_div(BPS_DENOMINATOR))
        .ok_or(ProgramError::ArithmeticOverflow)?;

    let sol_amount_after_fee = sol_amount
        .checked_sub(fee)
        .ok_or(ProgramError::ArithmeticOverflow)?;

    // Calculate tokens for reserve if reserve_fee > 0 (must happen BEFORE user purchase)
    let mut reserve_tokens_out: u64 = 0;
    if reserve_fee > 0 {
        // Use reserve fee to buy tokens at the current rate
        let reserve_k = u128::from(curve.virtual_sol_reserves)
            .checked_mul(u128::from(curve.virtual_token_reserves))
            .ok_or(ProgramError::ArithmeticOverflow)?;
        let reserve_new_virtual_sol_reserves = curve
            .virtual_sol_reserves
            .checked_add(reserve_fee)
            .ok_or(ProgramError::ArithmeticOverflow)?;
        let reserve_new_virtual_token_reserves = reserve_k
            .checked_div(u128::from(reserve_new_virtual_sol_reserves))
            .ok_or(ProgramError::ArithmeticOverflow)?;
        reserve_tokens_out = curve
            .virtual_token_reserves
            .checked_sub(reserve_new_virtual_token_reserves as u64)
            .ok_or(ProgramError::ArithmeticOverflow)?;
        
        // Update virtual reserves for reserve purchase
        curve.virtual_sol_reserves = reserve_new_virtual_sol_reserves;
        curve.virtual_token_reserves = reserve_new_virtual_token_reserves as u64;
        curve.real_sol_reserves = curve
            .real_sol_reserves
            .checked_add(reserve_fee)
            .ok_or(ProgramError::ArithmeticOverflow)?;
        curve.real_token_reserves = curve
            .real_token_reserves
            .checked_sub(reserve_tokens_out)
            .ok_or(ProgramError::ArithmeticOverflow)?;
    }

    // Calculate token output for user using the constant product formula
    let k = u128::from(curve.virtual_sol_reserves)
        .checked_mul(u128::from(curve.virtual_token_reserves))
        .ok_or(ProgramError::ArithmeticOverflow)?;

    let new_virtual_sol_reserves = curve
        .virtual_sol_reserves
        .checked_add(sol_amount_after_fee)
        .ok_or(ProgramError::ArithmeticOverflow)?;

    let new_virtual_token_reserves = k
        .checked_div(u128::from(new_virtual_sol_reserves))
        .ok_or(ProgramError::ArithmeticOverflow)?;

    let tokens_out = curve
        .virtual_token_reserves
        .checked_sub(new_virtual_token_reserves as u64)
        .ok_or(ProgramError::ArithmeticOverflow)?;

    require_gte!(tokens_out, min_token_output);
    require_gte!(curve.real_token_reserves, tokens_out);

    // State Updates for user purchase
    curve.virtual_sol_reserves = new_virtual_sol_reserves;
    curve.virtual_token_reserves = new_virtual_token_reserves as u64;
    curve.real_sol_reserves = curve
        .real_sol_reserves
        .checked_add(sol_amount_after_fee)
        .ok_or(ProgramError::ArithmeticOverflow)?;
    curve.real_token_reserves = curve
        .real_token_reserves
        .checked_sub(tokens_out)
        .ok_or(ProgramError::ArithmeticOverflow)?;

    // SOL Transfers (CPIs)
    // Platform fee always goes to platform_fee_recipient
    if platform_fee > 0 {
        let platform_fee_transfer_cpi_context = CpiContext::new(
            ctx.accounts.system_program.to_account_info(),
            anchor_lang::system_program::Transfer {
                from: ctx.accounts.signer.to_account_info(),
                to: ctx.accounts.platform_fee_recipient.to_account_info(),
            },
        );
        anchor_lang::system_program::transfer(
            platform_fee_transfer_cpi_context,
            platform_fee,
        )?;
    }

    // All remaining SOL (user's portion + reserve fee) goes to bonding curve
    // Reserve fee is already accounted for in state updates above
    let total_sol_to_curve = sol_amount_after_fee
        .checked_add(reserve_fee)
        .ok_or(ProgramError::ArithmeticOverflow)?;
    let sol_transfer_cpi_context = CpiContext::new(
        ctx.accounts.system_program.to_account_info(),
        anchor_lang::system_program::Transfer {
            from: ctx.accounts.signer.to_account_info(),
            to: curve.to_account_info(),
        },
    );
    anchor_lang::system_program::transfer(sol_transfer_cpi_context, total_sol_to_curve)?;

    // Transfer reserve tokens to reserve ATA
    if reserve_fee > 0 {
        let bonding_curve_seeds: &[&[&[u8]]] = &[&[
            b"bonding_curve",
            ctx.accounts.mint.to_account_info().key.as_ref(),
            &[ctx.bumps.bonding_curve],
        ]];
        let decimals = ctx.accounts.mint.decimals;
        let reserve_cpi_accounts = TransferChecked {
            mint: ctx.accounts.mint.to_account_info(),
            from: ctx.accounts.bonding_curve_ata.to_account_info(),
            to: ctx.accounts.reserve_ata.to_account_info(),
            authority: curve.to_account_info(),
        };
        let reserve_cpi_program = ctx.accounts.token_program.to_account_info();
        let reserve_cpi_context = CpiContext::new(reserve_cpi_program, reserve_cpi_accounts)
            .with_signer(bonding_curve_seeds);
        token_interface::transfer_checked(reserve_cpi_context, reserve_tokens_out, decimals)?;
    }

    // Token Transfer (CPI) for user
    let seeds: &[&[&[u8]]] = &[&[
        b"bonding_curve",
        ctx.accounts.mint.to_account_info().key.as_ref(),
        &[ctx.bumps.bonding_curve],
    ]];
    let decimals = ctx.accounts.mint.decimals;
    let cpi_accounts = TransferChecked {
        mint: ctx.accounts.mint.to_account_info(),
        from: ctx.accounts.bonding_curve_ata.to_account_info(),
        to: ctx.accounts.user_ata.to_account_info(),
        authority: curve.to_account_info(),
    };
    let cpi_program = ctx.accounts.token_program.to_account_info();
    let cpi_context = CpiContext::new(cpi_program, cpi_accounts).with_signer(seeds);
    token_interface::transfer_checked(cpi_context, tokens_out, decimals)?;

    // Emit trade event
    emit!(Trade {
        mint: ctx.accounts.mint.key(),
        trader: ctx.accounts.signer.key(),
        side: TradeSide::Buy,
        sol_amount,
        token_amount: tokens_out,
    });

    // Check for graduation
    if curve.real_sol_reserves >= ctx.accounts.global.graduation_threshold {
        curve.complete = true;
        msg!("Bonding curve has graduated!");
        
        // Emit curve complete event
        emit!(CurveComplete {
            mint: ctx.accounts.mint.key(),
            bonding_curve: ctx.accounts.bonding_curve.key(),
        });
    }

    Ok(())
}
