use anchor_lang::prelude::*;
use crate::states::Global;
use crate::errors::ErrorCode;
use crate::consts::MAX_FEE_BPS;

#[derive(Accounts)]
pub struct UpdateGlobalConfig<'info> {
    #[account(mut)]
    pub authority: Signer<'info>,
    #[account(
        mut,
        seeds = [b"global"],
        bump,
        constraint = global.authority == authority.key()
    )]
    pub global: Account<'info, Global>,
}

#[allow(clippy::too_many_arguments)]
pub fn handler(
    ctx: Context<UpdateGlobalConfig>,
    new_authority: Pubkey,
    new_platform_fee_recipient: Pubkey,
    new_platform_trade_fee_bps: u64,
    new_reserve_trade_fee_bps: u64,
    new_initial_virtual_token_reserves: u64,
    new_initial_virtual_sol_reserves: u64,
    new_token_total_supply: u64,
    new_graduation_threshold: u64,
) -> Result<()> {
    // Enforce 30% maximum fee cap
    require!(
        new_platform_trade_fee_bps
            .checked_add(new_reserve_trade_fee_bps)
            .unwrap_or(u64::MAX)
            <= MAX_FEE_BPS,
        ErrorCode::FeeTooHigh
    );

    ctx.accounts.global.set_inner(Global {
        authority: new_authority,
        platform_fee_recipient: new_platform_fee_recipient,
        reserve: ctx.accounts.global.reserve, // Keep existing reserve PDA
        initial_virtual_token_reserves: new_initial_virtual_token_reserves,
        initial_virtual_sol_reserves: new_initial_virtual_sol_reserves,
        token_total_supply: new_token_total_supply,
        platform_trade_fee_bps: new_platform_trade_fee_bps,
        reserve_trade_fee_bps: new_reserve_trade_fee_bps,
        graduation_threshold: new_graduation_threshold,
    });

    Ok(())
}
