// fixes unexpected `cfg` errors
// check https://solana.stackexchange.com/questions/17777/unexpected-cfg-condition-value-solana
#![allow(unexpected_cfgs)]
use anchor_lang::prelude::*;

mod states;
mod errors;
mod events;
mod consts;
mod instructions;

use instructions::*;

declare_id!("ihC7UqkLYWxQKVuYLiWNGqGvQCZb2ih4DXMLfyM6F68");

#[program]
pub mod coinfun {
    use super::*;

    pub fn initialize(
        ctx: Context<Initialize>,
        initial_virtual_token_reserves: u64,
        initial_virtual_sol_reserves: u64,
        token_total_supply: u64,
        platform_trade_fee_bps: u64,
        reserve_trade_fee_bps: u64,
        platform_fee_recipient: Pubkey,
        graduation_threshold: u64,
    ) -> Result<()> {
        instructions::initialize::handler(
            ctx,
            initial_virtual_token_reserves,
            initial_virtual_sol_reserves,
            token_total_supply,
            platform_trade_fee_bps,
            reserve_trade_fee_bps,
            platform_fee_recipient,
            graduation_threshold,
        )
    }

    pub fn create(
        ctx: Context<Create>,
        token_name: String,
        token_symbol: String,
        token_uri: String,
    ) -> Result<()> {
        instructions::create::handler(ctx, token_name, token_symbol, token_uri)
    }

    pub fn buy(ctx: Context<Buy>, sol_amount: u64, min_token_output: u64) -> Result<()> {
        instructions::buy::handler(ctx, sol_amount, min_token_output)
    }

    pub fn sell(ctx: Context<Sell>, token_amount: u64, min_sol_output: u64) -> Result<()> {
        instructions::sell::handler(ctx, token_amount, min_sol_output)
    }

    pub fn withdraw(ctx: Context<Withdraw>) -> Result<()> {
        instructions::withdraw::handler(ctx)
    }

    pub fn withdraw_reserve(ctx: Context<WithdrawReserve>, amount: u64) -> Result<()> {
        instructions::withdraw_reserve::handler(ctx, amount)
    }

    pub fn deposit_to_reserve(ctx: Context<DepositToReserve>, amount: u64) -> Result<()> {
        instructions::deposit_to_reserve::handler(ctx, amount)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn update_global_config(
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
        instructions::update_global_config::handler(
            ctx,
            new_authority,
            new_platform_fee_recipient,
            new_platform_trade_fee_bps,
            new_reserve_trade_fee_bps,
            new_initial_virtual_token_reserves,
            new_initial_virtual_sol_reserves,
            new_token_total_supply,
            new_graduation_threshold,
        )
    }
}
