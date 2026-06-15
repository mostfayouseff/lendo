use anyhow::Result;
use sqlx::PgPool;
use uuid::Uuid;

use crate::models::token::{CreateToken, Token, TokenStatus, UpdateToken};

#[derive(Clone)]
pub struct TokenRepository {
    pool: PgPool,
}

impl TokenRepository {
    pub fn new(pool: PgPool) -> Self { Self { pool } }

    pub async fn create(&self, req: &CreateToken) -> Result<Token> {
        let t = sqlx::query_as!(
            Token,
            r#"INSERT INTO tokens (mint_address, symbol, name, decimals, logo_uri, coingecko_id)
               VALUES ($1, $2, $3, $4, $5, $6)
               RETURNING id, mint_address, symbol, name, decimals,
                         status AS "status: TokenStatus",
                         logo_uri, coingecko_id, liquidity_usd, verified, created_at, updated_at"#,
            req.mint_address, req.symbol, req.name, req.decimals, req.logo_uri, req.coingecko_id,
        )
        .fetch_one(&self.pool)
        .await?;
        Ok(t)
    }

    pub async fn find_by_id(&self, id: Uuid) -> Result<Option<Token>> {
        let t = sqlx::query_as!(
            Token,
            r#"SELECT id, mint_address, symbol, name, decimals,
                      status AS "status: TokenStatus",
                      logo_uri, coingecko_id, liquidity_usd, verified, created_at, updated_at
               FROM tokens WHERE id = $1"#, id,
        )
        .fetch_optional(&self.pool)
        .await?;
        Ok(t)
    }

    pub async fn find_by_mint(&self, mint: &str) -> Result<Option<Token>> {
        let t = sqlx::query_as!(
            Token,
            r#"SELECT id, mint_address, symbol, name, decimals,
                      status AS "status: TokenStatus",
                      logo_uri, coingecko_id, liquidity_usd, verified, created_at, updated_at
               FROM tokens WHERE mint_address = $1"#, mint,
        )
        .fetch_optional(&self.pool)
        .await?;
        Ok(t)
    }

    pub async fn list(&self, limit: i64, offset: i64) -> Result<Vec<Token>> {
        let ts = sqlx::query_as!(
            Token,
            r#"SELECT id, mint_address, symbol, name, decimals,
                      status AS "status: TokenStatus",
                      logo_uri, coingecko_id, liquidity_usd, verified, created_at, updated_at
               FROM tokens ORDER BY symbol ASC LIMIT $1 OFFSET $2"#,
            limit, offset,
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(ts)
    }

    pub async fn list_active(&self) -> Result<Vec<Token>> {
        let ts = sqlx::query_as!(
            Token,
            r#"SELECT id, mint_address, symbol, name, decimals,
                      status AS "status: TokenStatus",
                      logo_uri, coingecko_id, liquidity_usd, verified, created_at, updated_at
               FROM tokens WHERE status = 'active' ORDER BY symbol ASC"#,
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(ts)
    }

    pub async fn update(&self, id: Uuid, req: &UpdateToken) -> Result<Option<Token>> {
        let t = sqlx::query_as!(
            Token,
            r#"UPDATE tokens
               SET symbol       = COALESCE($2, symbol),
                   name         = COALESCE($3, name),
                   status       = COALESCE($4, status),
                   logo_uri     = COALESCE($5, logo_uri),
                   coingecko_id = COALESCE($6, coingecko_id)
               WHERE id = $1
               RETURNING id, mint_address, symbol, name, decimals,
                         status AS "status: TokenStatus",
                         logo_uri, coingecko_id, liquidity_usd, verified, created_at, updated_at"#,
            id, req.symbol, req.name, req.status as Option<TokenStatus>,
            req.logo_uri, req.coingecko_id,
        )
        .fetch_optional(&self.pool)
        .await?;
        Ok(t)
    }

    pub async fn set_status(&self, id: Uuid, status: TokenStatus) -> Result<bool> {
        let r = sqlx::query!(
            "UPDATE tokens SET status = $2 WHERE id = $1",
            id, status as TokenStatus,
        )
        .execute(&self.pool).await?;
        Ok(r.rows_affected() > 0)
    }

    pub async fn delete(&self, id: Uuid) -> Result<bool> {
        let r = sqlx::query!("DELETE FROM tokens WHERE id = $1", id)
            .execute(&self.pool).await?;
        Ok(r.rows_affected() > 0)
    }

    pub async fn count(&self) -> Result<i64> {
        let row = sqlx::query!("SELECT COUNT(*) as cnt FROM tokens")
            .fetch_one(&self.pool).await?;
        Ok(row.cnt.unwrap_or(0))
    }
}
