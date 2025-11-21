use anchor_lang::prelude::*;
use crate::states::Global;
use crate::errors::ErrorCode;
use crate::consts::MAX_FEE_BPS;

#[derive(Accounts)]
pub struct Initialize<'info> {
    #[account(mut)]
    pub authority: Signer<'info>,
    #[account(
        init,
        payer = authority,
        space = 8 + Global::INIT_SPACE,
        seeds = [b"global"], bump
    )]
    pub global: Account<'info, Global>,
    /// CHECK: Global reserve PDA (authority for all reserve ATAs)
    #[account(
        init,
        payer = authority,
        space = 8,
        seeds = [b"reserve"], bump
    )]
    pub global_reserve: UncheckedAccount<'info>,
    pub system_program: Program<'info, System>,
}

pub fn handler(
    ctx: Context<Initialize>,
    initial_virtual_token_reserves: u64,
    initial_virtual_sol_reserves: u64,
    token_total_supply: u64,
    platform_trade_fee_bps: u64,
    reserve_trade_fee_bps: u64,
    platform_fee_recipient: Pubkey,
    graduation_threshold: u64,
) -> Result<()> {
    // Enforce 30% maximum fee cap
    require!(
        platform_trade_fee_bps
            .checked_add(reserve_trade_fee_bps)
            .unwrap_or(u64::MAX)
            <= MAX_FEE_BPS,
        ErrorCode::FeeTooHigh
    );
    ctx.accounts.global.set_inner(Global {
        authority: ctx.accounts.authority.key(),
        platform_fee_recipient,
        reserve: ctx.accounts.global_reserve.key(), // Global reserve PDA address
        initial_virtual_token_reserves,
        initial_virtual_sol_reserves,
        token_total_supply,
        platform_trade_fee_bps,
        reserve_trade_fee_bps,
        graduation_threshold,
    });
    
    Ok(())
}
