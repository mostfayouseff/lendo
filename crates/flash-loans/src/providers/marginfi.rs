use async_trait::async_trait;
use serde_json::json;
use tracing::info;

use crate::{
    error::FlashLoanError,
    providers::FlashLoanProvider,
    types::{FlashLoanParams, FlashLoanReceipt, FlashLoanRequest},
};

pub const MARGINFI_PROGRAM_ID: &str = "MFv2hWf31Z9kbCa1snEPdcgp7oziCMSf8xXGNRqJmMx";
pub const MARGINFI_FEE_BPS:    u16  = 0;

#[derive(Clone)]
pub struct MarginFiProvider {
    _rpc_url: String,
    client:  reqwest::Client,
}

impl MarginFiProvider {
    pub fn new(rpc_url: &str) -> Self {
        Self { _rpc_url: rpc_url.to_string(), client: reqwest::Client::new() }
    }
}

#[async_trait]
impl FlashLoanProvider for MarginFiProvider {
    fn name(&self) -> &str { "marginfi" }

    async fn get_params(&self, mint: &str, amount: u64) -> Result<FlashLoanParams, FlashLoanError> {
        let fee_amount   = amount * MARGINFI_FEE_BPS as u64 / 10_000;
        let repay_amount = amount + fee_amount;

        info!(provider = "marginfi", mint, amount, fee_amount, "Flash loan params computed");

        Ok(FlashLoanParams {
            provider:      "marginfi".to_string(),
            borrow_mint:   mint.to_string(),
            borrow_amount: amount,
            fee_amount,
            fee_bps:       MARGINFI_FEE_BPS,
            repay_amount,
            pool_reserve:  String::new(),
            destination:   String::new(),
            extra:         json!({ "program_id": MARGINFI_PROGRAM_ID }),
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
        info!(provider = "marginfi", profit = estimated_profit, "Flash loan receipt built");
        Ok(FlashLoanReceipt {
            provider:          "marginfi".to_string(),
            borrow_mint:       req.borrow_mint.clone(),
            borrow_amount:     params.borrow_amount,
            repay_amount:      params.repay_amount,
            fee_amount:        params.fee_amount,
            fee_bps:           params.fee_bps,
            estimated_profit,
            transaction_bytes: Vec::new(),
        })
    }

    async fn check_liquidity(&self, _mint: &str, _amount: u64) -> Result<bool, FlashLoanError> {
        Ok(true)
    }
}
