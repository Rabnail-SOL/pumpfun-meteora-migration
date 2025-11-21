use anchor_lang::prelude::*;

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
