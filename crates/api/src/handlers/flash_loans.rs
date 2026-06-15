use axum::{
    extract::{Extension, State},
    Json,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use auth::middleware::AuthUser;
use flash_loans::{providers::ProviderFactory, types::FlashLoanRequest};

use crate::{error::{ApiError, ApiResult}, state::AppState};

#[derive(Debug, Deserialize)]
pub struct FlashLoanQuoteRequest {
    pub provider:     String,
    pub borrow_mint:  String,
    pub borrow_amount: u64,
}

#[derive(Debug, Serialize)]
pub struct FlashLoanQuote {
    pub provider:      String,
    pub borrow_mint:   String,
    pub borrow_amount: u64,
    pub fee_amount:    u64,
    pub fee_bps:       u16,
    pub repay_amount:  u64,
    pub available:     bool,
}

pub async fn quote(
    State(state): State<Arc<AppState>>,
    Json(req): Json<FlashLoanQuoteRequest>,
) -> ApiResult<Json<FlashLoanQuote>> {
    let rpc_url = std::env::var("APEX_HTTP_RPC_URL")
        .unwrap_or_else(|_| "https://api.mainnet-beta.solana.com".to_string());

    let provider = ProviderFactory::create(&req.provider, &rpc_url)
        .map_err(|e| ApiError::BadRequest(e.to_string()))?;

    let available = provider.check_liquidity(&req.borrow_mint, req.borrow_amount)
        .await.unwrap_or(false);

    let params = provider.get_params(&req.borrow_mint, req.borrow_amount).await
        .map_err(|e| ApiError::Internal(anyhow::anyhow!(e.to_string())))?;

    Ok(Json(FlashLoanQuote {
        provider:      params.provider,
        borrow_mint:   params.borrow_mint,
        borrow_amount: params.borrow_amount,
        fee_amount:    params.fee_amount,
        fee_bps:       params.fee_bps,
        repay_amount:  params.repay_amount,
        available,
    }))
}

pub async fn providers() -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "providers": [
            { "name": "solend",   "fee_bps": 9, "description": "Solend main pool" },
            { "name": "marginfi", "fee_bps": 0, "description": "MarginFi zero-fee flash loans" },
            { "name": "kamino",   "fee_bps": 5, "description": "Kamino lending" },
        ]
    }))
}
