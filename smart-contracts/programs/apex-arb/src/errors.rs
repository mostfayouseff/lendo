use anchor_lang::prelude::*;

#[error_code]
pub enum ApexError {
    #[msg("Arbitrage is not profitable after fees")]
    NotProfitable,
    #[msg("Slippage tolerance exceeded")]
    SlippageExceeded,
    #[msg("Flash loan repayment amount exceeds profit")]
    RepaymentExceedsProfit,
    #[msg("Invalid route data")]
    InvalidRouteData,
    #[msg("Protocol is currently paused")]
    ProtocolPaused,
    #[msg("Unauthorized: only owner can perform this action")]
    Unauthorized,
    #[msg("Arithmetic overflow")]
    Overflow,
    #[msg("Insufficient output balance after swap")]
    InsufficientOutput,
    #[msg("Maximum hops exceeded")]
    MaxHopsExceeded,
}
