use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Setting {
    pub key:         String,
    pub value:       serde_json::Value,
    pub description: Option<String>,
    pub updated_by:  Option<Uuid>,
    pub updated_at:  DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateSetting {
    pub key:   String,
    pub value: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BotSettings {
    pub enabled:               bool,
    pub mode:                  String,
    pub min_profit_lamports:   i64,
    pub max_position_lamports: i64,
    pub slippage_bps:          i16,
    pub max_hops:              i16,
    pub flash_loan_enabled:    bool,
    pub flash_loan_provider:   String,
    pub jito_tip_lamports:     i64,
}

impl Default for BotSettings {
    fn default() -> Self {
        Self {
            enabled:               false,
            mode:                  "test".to_string(),
            min_profit_lamports:   10_000,
            max_position_lamports: 1_000_000_000,
            slippage_bps:          50,
            max_hops:              4,
            flash_loan_enabled:    false,
            flash_loan_provider:   "solend".to_string(),
            jito_tip_lamports:     1_000,
        }
    }
}
