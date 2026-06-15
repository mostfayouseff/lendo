use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::Type;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Type, Serialize, Deserialize)]
#[sqlx(type_name = "event_severity", rename_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub enum EventSeverity { Debug, Info, Warning, Error, Critical }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Type, Serialize, Deserialize)]
#[sqlx(type_name = "event_category", rename_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub enum EventCategory {
    Rpc, Ingress, Trading, Risk, CircuitBreaker,
    FlashLoan, Wallet, Monitoring, System,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct SystemEvent {
    pub id:          Uuid,
    pub severity:    EventSeverity,
    pub category:    EventCategory,
    pub title:       String,
    pub message:     String,
    pub metadata:    serde_json::Value,
    pub resolved:    bool,
    pub resolved_at: Option<DateTime<Utc>>,
    pub created_at:  DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct CreateSystemEvent {
    pub severity: EventSeverity,
    pub category: EventCategory,
    pub title:    String,
    pub message:  String,
    pub metadata: serde_json::Value,
}
