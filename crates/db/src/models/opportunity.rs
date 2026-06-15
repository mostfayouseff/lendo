use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::Type;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Type, Serialize, Deserialize)]
#[sqlx(type_name = "opportunity_status", rename_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub enum OpportunityStatus {
    Detected,
    Simulating,
    Executing,
    Executed,
    Skipped,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Opportunity {
    pub id:                       Uuid,
    pub strategy_id:              Option<Uuid>,
    pub status:                   OpportunityStatus,
    pub path:                     Vec<String>,
    pub dex_path:                 String,
    pub input_mint:               String,
    pub output_mint:              String,
    pub input_amount_lamports:    i64,
    pub estimated_profit_lamports: i64,
    pub estimated_profit_usd:     Option<sqlx::types::Decimal>,
    pub price_impact_pct:         Option<sqlx::types::Decimal>,
    pub hop_count:                i16,
    pub gnn_confidence:           Option<sqlx::types::Decimal>,
    pub skip_reason:              Option<String>,
    pub detected_at:              DateTime<Utc>,
    pub executed_at:              Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateOpportunity {
    pub strategy_id:              Option<Uuid>,
    pub path:                     Vec<String>,
    pub dex_path:                 String,
    pub input_mint:               String,
    pub output_mint:              String,
    pub input_amount_lamports:    i64,
    pub estimated_profit_lamports: i64,
    pub hop_count:                i16,
    pub gnn_confidence:           Option<f64>,
}
