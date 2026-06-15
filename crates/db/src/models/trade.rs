use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::Type;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Type, Serialize, Deserialize)]
#[sqlx(type_name = "trade_status", rename_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub enum TradeStatus {
    Pending,
    Simulating,
    Signed,
    Submitted,
    Confirmed,
    Failed,
    Reverted,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Trade {
    pub id:                       Uuid,
    pub opportunity_id:           Option<Uuid>,
    pub strategy_id:              Option<Uuid>,
    pub wallet_id:                Option<Uuid>,
    pub status:                   TradeStatus,
    pub signature:                Option<String>,
    pub input_mint:               String,
    pub output_mint:              String,
    pub input_amount_lamports:    i64,
    pub output_amount_lamports:   Option<i64>,
    pub expected_profit_lamports: i64,
    pub actual_profit_lamports:   Option<i64>,
    pub fee_lamports:             i64,
    pub jito_tip_lamports:        i64,
    pub flash_loan_fee_lamports:  i64,
    pub slippage_bps:             Option<i16>,
    pub hop_count:                i16,
    pub dex_path:                 String,
    pub simulation_passed:        Option<bool>,
    pub error_message:            Option<String>,
    pub slot:                     Option<i64>,
    pub block_time:               Option<DateTime<Utc>>,
    pub created_at:               DateTime<Utc>,
    pub confirmed_at:             Option<DateTime<Utc>>,
    pub updated_at:               DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateTrade {
    pub opportunity_id:           Option<Uuid>,
    pub strategy_id:              Option<Uuid>,
    pub wallet_id:                Option<Uuid>,
    pub input_mint:               String,
    pub output_mint:              String,
    pub input_amount_lamports:    i64,
    pub expected_profit_lamports: i64,
    pub fee_lamports:             i64,
    pub jito_tip_lamports:        i64,
    pub flash_loan_fee_lamports:  i64,
    pub hop_count:                i16,
    pub dex_path:                 String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TradeSummary {
    pub total_trades:         i64,
    pub confirmed_trades:     i64,
    pub failed_trades:        i64,
    pub total_profit_lamports: i64,
    pub total_profit_sol:      f64,
    pub win_rate:              f64,
    pub avg_profit_lamports:   f64,
}
