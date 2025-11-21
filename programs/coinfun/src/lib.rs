// fixes unexpected `cfg` errors
// check https://solana.stackexchange.com/questions/17777/unexpected-cfg-condition-value-solana
#![allow(unexpected_cfgs)]
use anchor_lang::prelude::*;
use anchor_spl::{
    associated_token::AssociatedToken,
    metadata::{
        create_metadata_accounts_v3, mpl_token_metadata::types::DataV2, CreateMetadataAccountsV3,
        Metadata,
    },
    token_interface::{self, Mint, MintTo, TokenAccount, TokenInterface, TransferChecked},
};

mod account;
mod error;
use account::*;

declare_id!("9oAMNonh6hoAru6fBsKbjtY13GeX3PwzsV49bTPfbxX3");

#[program]
pub mod coinfun {

    use crate::error::ErrorCode;

    use super::*;

    #[allow(clippy::too_many_arguments)]
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
        // Enforce 30% maximum fee cap
        require!(
            platform_trade_fee_bps
                .checked_add(reserve_trade_fee_bps)
                .unwrap_or(u64::MAX)
                <= 3000,
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

    pub fn create(
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

    pub fn buy(ctx: Context<Buy>, sol_amount: u64, min_token_output: u64) -> Result<()> {
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
            .and_then(|res| res.checked_div(10000))
            .ok_or(ProgramError::ArithmeticOverflow)?;

        // Split fees between platform and reserve
        let platform_fee = sol_amount
            .checked_mul(ctx.accounts.global.platform_trade_fee_bps)
            .and_then(|res| res.checked_div(10000))
            .ok_or(ProgramError::ArithmeticOverflow)?;
        let reserve_fee = sol_amount
            .checked_mul(ctx.accounts.global.reserve_trade_fee_bps)
            .and_then(|res| res.checked_div(10000))
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

    pub fn sell(ctx: Context<Sell>, token_amount: u64, min_sol_output: u64) -> Result<()> {
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
            .and_then(|res| res.checked_div(10000))
            .ok_or(ProgramError::ArithmeticOverflow)?;

        let platform_fee = sol_out_gross
            .checked_mul(ctx.accounts.global.platform_trade_fee_bps)
            .and_then(|res| res.checked_div(10000))
            .ok_or(ProgramError::ArithmeticOverflow)?;
        let reserve_fee = sol_out_gross
            .checked_mul(ctx.accounts.global.reserve_trade_fee_bps)
            .and_then(|res| res.checked_div(10000))
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

    pub fn withdraw(ctx: Context<Withdraw>) -> Result<()> {
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

    pub fn withdraw_reserve(ctx: Context<WithdrawReserve>, amount: u64) -> Result<()> {
        // Withdraw specified amount from the reserve ATA
        let token_balance = ctx.accounts.reserve_ata.amount;
        require!(token_balance > 0, ErrorCode::NothingToWithdraw);
        require!(amount > 0, ErrorCode::NothingToWithdraw);
        require!(amount <= token_balance, ErrorCode::NothingToWithdraw);

        let decimals = ctx.accounts.mint.decimals;
        // Use global reserve PDA as authority (seeded with ["reserve"])
        let global_reserve_seeds: &[&[&[u8]]] = &[&[
            b"reserve",
            &[ctx.bumps.global_reserve],
        ]];
        let cpi_accounts = TransferChecked {
            mint: ctx.accounts.mint.to_account_info(),
            from: ctx.accounts.reserve_ata.to_account_info(),
            to: ctx.accounts.authority_ata.to_account_info(),
            authority: ctx.accounts.global_reserve.to_account_info(),
        };
        let cpi_program = ctx.accounts.token_program.to_account_info();
        let cpi_context = CpiContext::new(cpi_program, cpi_accounts).with_signer(global_reserve_seeds);
        token_interface::transfer_checked(cpi_context, amount, decimals)?;

        Ok(())
    }

    pub fn deposit_to_reserve(ctx: Context<DepositToReserve>, amount: u64) -> Result<()> {
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
        // Enforce 30% maximum fee cap
        require!(
            new_platform_trade_fee_bps
                .checked_add(new_reserve_trade_fee_bps)
                .unwrap_or(u64::MAX)
                <= 3000,
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
}

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

#[derive(Accounts)]
pub struct WithdrawReserve<'info> {
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
    pub system_program: Program<'info, System>,
    pub token_program: Interface<'info, TokenInterface>,
    pub associated_token_program: Program<'info, AssociatedToken>,
}

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

// Events
#[derive(AnchorSerialize, AnchorDeserialize)]
pub enum TradeSide {
    Buy,
    Sell,
}

#[event]
pub struct TokenCreated {
    pub mint: Pubkey,
    pub creator: Pubkey,
}

#[event]
pub struct Trade {
    pub mint: Pubkey,
    pub trader: Pubkey,
    pub side: TradeSide,
    pub sol_amount: u64,
    pub token_amount: u64,
}

#[event]
pub struct CurveComplete {
    pub mint: Pubkey,
    pub bonding_curve: Pubkey,
}
