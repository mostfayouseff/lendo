use anyhow::Result;
use sqlx::PgPool;
use uuid::Uuid;

use crate::models::wallet::{CreateWallet, UpdateWallet, Wallet, WalletStatus, WalletType};

#[derive(Clone)]
pub struct WalletRepository {
    pool: PgPool,
}

impl WalletRepository {
    pub fn new(pool: PgPool) -> Self { Self { pool } }

    pub async fn create(&self, user_id: Uuid, req: &CreateWallet, address: &str, encrypted: &str) -> Result<Wallet> {
        let w = sqlx::query_as!(
            Wallet,
            r#"INSERT INTO wallets (user_id, label, address, wallet_type, encrypted_secret)
               VALUES ($1, $2, $3, $4, $5)
               RETURNING id, user_id, label, address,
                         wallet_type AS "wallet_type: WalletType",
                         encrypted_secret,
                         status AS "status: WalletStatus",
                         is_active, balance_lamports, balance_updated_at,
                         created_at, updated_at"#,
            user_id, req.label, address,
            req.wallet_type as WalletType, encrypted,
        )
        .fetch_one(&self.pool)
        .await?;
        Ok(w)
    }

    pub async fn find_by_id(&self, id: Uuid) -> Result<Option<Wallet>> {
        let w = sqlx::query_as!(
            Wallet,
            r#"SELECT id, user_id, label, address,
                      wallet_type AS "wallet_type: WalletType",
                      encrypted_secret,
                      status AS "status: WalletStatus",
                      is_active, balance_lamports, balance_updated_at, created_at, updated_at
               FROM wallets WHERE id = $1"#, id,
        )
        .fetch_optional(&self.pool)
        .await?;
        Ok(w)
    }

    pub async fn list_by_user(&self, user_id: Uuid) -> Result<Vec<Wallet>> {
        let ws = sqlx::query_as!(
            Wallet,
            r#"SELECT id, user_id, label, address,
                      wallet_type AS "wallet_type: WalletType",
                      encrypted_secret,
                      status AS "status: WalletStatus",
                      is_active, balance_lamports, balance_updated_at, created_at, updated_at
               FROM wallets WHERE user_id = $1 ORDER BY created_at DESC"#, user_id,
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(ws)
    }

    pub async fn find_active(&self, user_id: Uuid) -> Result<Option<Wallet>> {
        let w = sqlx::query_as!(
            Wallet,
            r#"SELECT id, user_id, label, address,
                      wallet_type AS "wallet_type: WalletType",
                      encrypted_secret,
                      status AS "status: WalletStatus",
                      is_active, balance_lamports, balance_updated_at, created_at, updated_at
               FROM wallets WHERE user_id = $1 AND is_active = TRUE LIMIT 1"#, user_id,
        )
        .fetch_optional(&self.pool)
        .await?;
        Ok(w)
    }

    pub async fn activate(&self, user_id: Uuid, wallet_id: Uuid) -> Result<()> {
        let mut tx = self.pool.begin().await?;
        sqlx::query!("UPDATE wallets SET is_active = FALSE WHERE user_id = $1", user_id)
            .execute(&mut *tx).await?;
        sqlx::query!(
            "UPDATE wallets SET is_active = TRUE, status = 'active' WHERE id = $1 AND user_id = $2",
            wallet_id, user_id,
        )
        .execute(&mut *tx).await?;
        tx.commit().await?;
        Ok(())
    }

    pub async fn update_balance(&self, id: Uuid, balance: i64) -> Result<()> {
        sqlx::query!(
            "UPDATE wallets SET balance_lamports = $2, balance_updated_at = NOW() WHERE id = $1",
            id, balance,
        )
        .execute(&self.pool).await?;
        Ok(())
    }

    pub async fn update(&self, id: Uuid, req: &UpdateWallet) -> Result<Option<Wallet>> {
        let w = sqlx::query_as!(
            Wallet,
            r#"UPDATE wallets SET label = COALESCE($2, label), status = COALESCE($3, status)
               WHERE id = $1
               RETURNING id, user_id, label, address,
                         wallet_type AS "wallet_type: WalletType",
                         encrypted_secret,
                         status AS "status: WalletStatus",
                         is_active, balance_lamports, balance_updated_at, created_at, updated_at"#,
            id, req.label, req.status as Option<WalletStatus>,
        )
        .fetch_optional(&self.pool)
        .await?;
        Ok(w)
    }

    pub async fn delete(&self, id: Uuid) -> Result<bool> {
        let r = sqlx::query!("DELETE FROM wallets WHERE id = $1", id)
            .execute(&self.pool).await?;
        Ok(r.rows_affected() > 0)
    }
}
