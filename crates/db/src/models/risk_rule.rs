use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::Type;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Type, Serialize, Deserialize)]
#[sqlx(type_name = "risk_rule_type", rename_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub enum RiskRuleType {
    PoolBlacklist,
    TokenBlacklist,
    DexBlacklist,
    MaxDailyLoss,
    MaxTradeSize,
    MaxSlippage,
    MaxConsecutiveLosses,
    WalletExposureLimit,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct RiskRule {
    pub id:          Uuid,
    pub rule_type:   RiskRuleType,
    pub enabled:     bool,
    pub name:        String,
    pub description: Option<String>,
    pub config:      serde_json::Value,
    pub created_by:  Option<Uuid>,
    pub created_at:  DateTime<Utc>,
    pub updated_at:  DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateRiskRule {
    pub rule_type:   RiskRuleType,
    pub name:        String,
    pub description: Option<String>,
    pub config:      serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateRiskRule {
    pub enabled:     Option<bool>,
    pub name:        Option<String>,
    pub description: Option<String>,
    pub config:      Option<serde_json::Value>,
}
