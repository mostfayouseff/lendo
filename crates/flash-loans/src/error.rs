use thiserror::Error;

#[derive(Debug, Error)]
pub enum FlashLoanError {
    #[error("Provider {0} is unavailable")]
    ProviderUnavailable(String),
    #[error("Insufficient pool liquidity: requested {requested}, available {available}")]
    InsufficientLiquidity { requested: u64, available: u64 },
    #[error("Repayment would exceed profit: loan {loan} + fee {fee} > output {output}")]
    UnprofitableAfterRepayment { loan: u64, fee: u64, output: u64 },
    #[error("Transaction construction failed: {0}")]
    TransactionBuild(String),
    #[error("Simulation failed: {0}")]
    SimulationFailed(String),
    #[error("RPC error: {0}")]
    RpcError(String),
    #[error("Invalid params: {0}")]
    InvalidParams(String),
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
}
