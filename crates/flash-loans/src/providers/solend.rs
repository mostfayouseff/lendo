use async_trait::async_trait;
use serde_json::{json, Value};
use tracing::{debug, info};

use crate::{
    error::FlashLoanError,
    providers::FlashLoanProvider,
    types::{FlashLoanParams, FlashLoanReceipt, FlashLoanRequest},
};

pub const SOLEND_PROGRAM_ID:     &str = "So1endDq2YkqhipRh3WViPa8hdiSpxWy6z3Z6tMCpAo";
pub const SOLEND_FEE_BPS:        u16  = 9;
pub const SOLEND_MAIN_POOL_SOL:  &str = "8PbodeaosQP19SjYFx855UMqWxH2HynZLdBXmsrbac36";
pub const SOLEND_MAIN_POOL_USDC: &str = "BgxfHJDzm44T7XG68MYKx7YisTjZu73tVovyZSjJMpmw";

#[derive(Clone)]
pub struct SolendProvider {
    rpc_url: String,
    client:  reqwest::Client,
}

impl SolendProvider {
    pub fn new(rpc_url: &str) -> Self {
        Self { rpc_url: rpc_url.to_string(), client: reqwest::Client::new() }
    }

    fn reserve_for_mint(&self, mint: &str) -> &'static str {
        match mint {
            "So11111111111111111111111111111111111111112"      => SOLEND_MAIN_POOL_SOL,
            "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v" => SOLEND_MAIN_POOL_USDC,
            _ => SOLEND_MAIN_POOL_SOL,
        }
    }

    async fn get_reserve_info(&self, reserve: &str) -> Result<Value, FlashLoanError> {
        let body = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "getAccountInfo",
            "params": [reserve, {"encoding": "base64"}]
        });
        let resp = self.client.post(&self.rpc_url).json(&body).send().await?;
        let val: Value = resp.json().await?;
        Ok(val)
    }
}

#[async_trait]
impl FlashLoanProvider for SolendProvider {
    fn name(&self) -> &str { "solend" }

    async fn get_params(&self, mint: &str, amount: u64) -> Result<FlashLoanParams, FlashLoanError> {
        let reserve = self.reserve_for_mint(mint);
        debug!(mint, amount, reserve, "Fetching Solend flash loan params");

        let fee_amount  = amount * SOLEND_FEE_BPS as u64 / 10_000;
        let repay_amount = amount + fee_amount;

        info!(
            provider = "solend", mint, amount, fee_amount, repay_amount,
            "Flash loan params computed"
        );

        Ok(FlashLoanParams {
            provider:      "solend".to_string(),
            borrow_mint:   mint.to_string(),
            borrow_amount: amount,
            fee_amount,
            fee_bps:       SOLEND_FEE_BPS,
            repay_amount,
            pool_reserve:  reserve.to_string(),
            destination:   String::new(),
            extra:         json!({
                "program_id": SOLEND_PROGRAM_ID,
                "reserve":    reserve,
            }),
        })
    }

    async fn build_transaction(&self, req: &FlashLoanRequest, params: &FlashLoanParams) -> Result<FlashLoanReceipt, FlashLoanError> {
        if req.expected_output <= params.repay_amount {
            return Err(FlashLoanError::UnprofitableAfterRepayment {
                loan:   params.borrow_amount,
                fee:    params.fee_amount,
                output: req.expected_output,
            });
        }

        let estimated_profit = req.expected_output as i64 - params.repay_amount as i64;

        info!(
            provider = "solend",
            borrow  = params.borrow_amount,
            repay   = params.repay_amount,
            profit  = estimated_profit,
            "Flash loan transaction built (simulation mode — attach on-chain program in production)"
        );

        Ok(FlashLoanReceipt {
            provider:          "solend".to_string(),
            borrow_mint:       req.borrow_mint.clone(),
            borrow_amount:     params.borrow_amount,
            repay_amount:      params.repay_amount,
            fee_amount:        params.fee_amount,
            fee_bps:           params.fee_bps,
            estimated_profit,
            transaction_bytes: Vec::new(),
        })
    }

    async fn check_liquidity(&self, mint: &str, amount: u64) -> Result<bool, FlashLoanError> {
        let reserve = self.reserve_for_mint(mint);
        let info = self.get_reserve_info(reserve).await?;
        if info.get("error").is_some() {
            return Ok(false);
        }
        Ok(amount <= 10_000_000_000_000)
    }
}
