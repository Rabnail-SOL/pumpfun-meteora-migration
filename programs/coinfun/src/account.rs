use anchor_lang::prelude::*;

#[account]
#[derive(InitSpace)]
pub struct Global {
    pub authority: Pubkey,
    pub platform_fee_recipient: Pubkey,
    pub reserve: Pubkey, // Global reserve PDA (authority for all reserve ATAs)
    pub initial_virtual_token_reserves: u64,
    pub initial_virtual_sol_reserves: u64,
    pub token_total_supply: u64,
    pub platform_trade_fee_bps: u64,
    pub reserve_trade_fee_bps: u64,
    pub graduation_threshold: u64,
}

#[account]
#[derive(InitSpace)]
pub struct BondingCurve {
    pub mint: Pubkey,
    pub creator: Pubkey,
    pub virtual_token_reserves: u64,
    pub virtual_sol_reserves: u64,
    pub real_token_reserves: u64,
    pub real_sol_reserves: u64,
    pub token_total_supply: u64,
    pub complete: bool,
}
