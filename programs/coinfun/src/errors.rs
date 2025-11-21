use anchor_lang::prelude::*;

#[error_code]
pub enum ErrorCode {
    #[msg("Bonding curve is complete")]
    BondingCurveComplete,
    #[msg("You are not the vault owner")]
    Unauthorized,
    #[msg("Nothing to withdraw")]
    NothingToWithdraw,
    #[msg("Bonding curve is not complete")]
    BondingCurveNotComplete,
    #[msg("Fee basis points cannot exceed 3000 (30%)")]
    FeeTooHigh,
    #[msg("Total supply must be greater than the initial real token reserves.")]
    InvalidTokenReserveConfiguration,
}
