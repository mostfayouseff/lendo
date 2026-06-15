use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::Type;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Type, Serialize, Deserialize)]
#[sqlx(type_name = "audit_action", rename_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub enum AuditAction {
    UserLogin, UserLogout, UserCreated, UserUpdated, UserDeleted,
    WalletAdded, WalletActivated, WalletDeleted,
    TokenAdded, TokenEnabled, TokenDisabled, TokenDeleted,
    StrategyCreated, StrategyUpdated, StrategyDeleted, StrategyStarted, StrategyPaused,
    BotStarted, BotStopped, BotPaused, BotResumed, EmergencyStop,
    SettingsUpdated, RiskRuleUpdated,
    TradeExecuted, TradeFailed,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct AuditLog {
    pub id:          Uuid,
    pub user_id:     Option<Uuid>,
    pub action:      AuditAction,
    pub entity_type: Option<String>,
    pub entity_id:   Option<Uuid>,
    pub old_value:   Option<serde_json::Value>,
    pub new_value:   Option<serde_json::Value>,
    pub ip_address:  Option<String>,
    pub user_agent:  Option<String>,
    pub metadata:    serde_json::Value,
    pub created_at:  DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct CreateAuditLog {
    pub user_id:     Option<Uuid>,
    pub action:      AuditAction,
    pub entity_type: Option<String>,
    pub entity_id:   Option<Uuid>,
    pub old_value:   Option<serde_json::Value>,
    pub new_value:   Option<serde_json::Value>,
    pub ip_address:  Option<String>,
    pub user_agent:  Option<String>,
    pub metadata:    serde_json::Value,
}
