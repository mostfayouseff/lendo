use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::Type;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Type, Serialize, Deserialize)]
#[sqlx(type_name = "strategy_type", rename_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub enum StrategyType {
    CrossDex,
    Triangular,
    MultiHop,
    FlashLoan,
    JupiterRoute,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Type, Serialize, Deserialize)]
#[sqlx(type_name = "strategy_status", rename_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub enum StrategyStatus {
    Active,
    Paused,
    Disabled,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Strategy {
    pub id:                    Uuid,
    pub user_id:               Uuid,
    pub name:                  String,
    pub strategy_type:         StrategyType,
    pub status:                StrategyStatus,
    pub min_profit_lamports:   i64,
    pub max_position_lamports: i64,
    pub max_slippage_bps:      i16,
    pub max_hops:              i16,
    pub flash_loan_enabled:    bool,
    pub flash_loan_provider:   Option<String>,
    pub dex_whitelist:         Vec<String>,
    pub token_whitelist:       Vec<String>,
    pub config:                serde_json::Value,
    pub trades_executed:       i64,
    pub total_profit_lamports: i64,
    pub created_at:            DateTime<Utc>,
    pub updated_at:            DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateStrategy {
    pub name:                  String,
    pub strategy_type:         StrategyType,
    pub min_profit_lamports:   i64,
    pub max_position_lamports: i64,
    pub max_slippage_bps:      i16,
    pub max_hops:              i16,
    pub flash_loan_enabled:    bool,
    pub flash_loan_provider:   Option<String>,
    pub dex_whitelist:         Vec<String>,
    pub token_whitelist:       Vec<String>,
    pub config:                Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UpdateStrategy {
    pub name:                  Option<String>,
    pub status:                Option<StrategyStatus>,
    pub min_profit_lamports:   Option<i64>,
    pub max_position_lamports: Option<i64>,
    pub max_slippage_bps:      Option<i16>,
    pub max_hops:              Option<i16>,
    pub flash_loan_enabled:    Option<bool>,
    pub flash_loan_provider:   Option<String>,
    pub dex_whitelist:         Option<Vec<String>>,
    pub token_whitelist:       Option<Vec<String>>,
    pub config:                Option<serde_json::Value>,
}
