use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlashLoanRequest {
    pub provider:            String,
    pub borrow_mint:         String,
    pub borrow_amount:       u64,
    pub arb_path:            Vec<String>,
    pub dex_path:            Vec<String>,
    pub expected_output:     u64,
    pub slippage_bps:        u16,
    pub wallet_pubkey:       String,
    pub jupiter_quote_token: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlashLoanParams {
    pub provider:       String,
    pub borrow_mint:    String,
    pub borrow_amount:  u64,
    pub fee_amount:     u64,
    pub fee_bps:        u16,
    pub repay_amount:   u64,
    pub pool_reserve:   String,
    pub destination:    String,
    pub extra:          serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlashLoanReceipt {
    pub provider:           String,
    pub borrow_mint:        String,
    pub borrow_amount:      u64,
    pub repay_amount:       u64,
    pub fee_amount:         u64,
    pub fee_bps:            u16,
    pub estimated_profit:   i64,
    pub transaction_bytes:  Vec<u8>,
}
