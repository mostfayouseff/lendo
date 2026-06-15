use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::Type;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Type, Serialize, Deserialize)]
#[sqlx(type_name = "token_status", rename_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub enum TokenStatus {
    Active,
    Disabled,
    Blacklisted,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Token {
    pub id:            Uuid,
    pub mint_address:  String,
    pub symbol:        String,
    pub name:          String,
    pub decimals:      i16,
    pub status:        TokenStatus,
    pub logo_uri:      Option<String>,
    pub coingecko_id:  Option<String>,
    pub liquidity_usd: Option<sqlx::types::Decimal>,
    pub verified:      bool,
    pub created_at:    DateTime<Utc>,
    pub updated_at:    DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateToken {
    pub mint_address: String,
    pub symbol:       String,
    pub name:         String,
    pub decimals:     i16,
    pub logo_uri:     Option<String>,
    pub coingecko_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateToken {
    pub symbol:       Option<String>,
    pub name:         Option<String>,
    pub status:       Option<TokenStatus>,
    pub logo_uri:     Option<String>,
    pub coingecko_id: Option<String>,
}
