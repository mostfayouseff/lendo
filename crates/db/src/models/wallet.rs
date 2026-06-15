use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::Type;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Type, Serialize, Deserialize)]
#[sqlx(type_name = "wallet_status", rename_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub enum WalletStatus {
    Active,
    Inactive,
    Locked,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Type, Serialize, Deserialize)]
#[sqlx(type_name = "wallet_type", rename_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub enum WalletType {
    KeypairJson,
    PrivateKey,
    SeedPhrase,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Wallet {
    pub id:                 Uuid,
    pub user_id:            Uuid,
    pub label:              String,
    pub address:            String,
    pub wallet_type:        WalletType,
    #[serde(skip_serializing)]
    pub encrypted_secret:   String,
    pub status:             WalletStatus,
    pub is_active:          bool,
    pub balance_lamports:   i64,
    pub balance_updated_at: Option<DateTime<Utc>>,
    pub created_at:         DateTime<Utc>,
    pub updated_at:         DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PublicWallet {
    pub id:                 Uuid,
    pub label:              String,
    pub address:            String,
    pub wallet_type:        WalletType,
    pub status:             WalletStatus,
    pub is_active:          bool,
    pub balance_lamports:   i64,
    pub balance_sol:        f64,
    pub balance_updated_at: Option<DateTime<Utc>>,
    pub created_at:         DateTime<Utc>,
}

impl From<Wallet> for PublicWallet {
    fn from(w: Wallet) -> Self {
        Self {
            id:                 w.id,
            label:              w.label,
            address:            w.address,
            wallet_type:        w.wallet_type,
            status:             w.status,
            is_active:          w.is_active,
            balance_lamports:   w.balance_lamports,
            balance_sol:        w.balance_lamports as f64 / 1e9,
            balance_updated_at: w.balance_updated_at,
            created_at:         w.created_at,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateWallet {
    pub label:           String,
    pub wallet_type:     WalletType,
    pub secret:          String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateWallet {
    pub label:  Option<String>,
    pub status: Option<WalletStatus>,
}
