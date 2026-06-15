use anyhow::Result;
use sqlx::PgPool;
use uuid::Uuid;

use crate::models::session::{CreateSession, Session};

#[derive(Clone)]
pub struct SessionRepository {
    pool: PgPool,
}

impl SessionRepository {
    pub fn new(pool: PgPool) -> Self { Self { pool } }

    pub async fn create(&self, req: &CreateSession) -> Result<Session> {
        let s = sqlx::query_as!(
            Session,
            r#"INSERT INTO sessions (user_id, refresh_token, ip_address, user_agent, expires_at)
               VALUES ($1, $2, $3, $4, $5)
               RETURNING id, user_id, refresh_token, ip_address,
                         user_agent, created_at, expires_at, revoked, revoked_at"#,
            req.user_id,
            req.refresh_token,
            req.ip_address,
            req.user_agent,
            req.expires_at,
        )
        .fetch_one(&self.pool)
        .await?;
        Ok(s)
    }

    pub async fn find_by_token(&self, token: &str) -> Result<Option<Session>> {
        let s = sqlx::query_as!(
            Session,
            r#"SELECT id, user_id, refresh_token, ip_address,
                      user_agent, created_at, expires_at, revoked, revoked_at
               FROM sessions WHERE refresh_token = $1 AND NOT revoked AND expires_at > NOW()"#,
            token,
        )
        .fetch_optional(&self.pool)
        .await?;
        Ok(s)
    }

    pub async fn revoke(&self, token: &str) -> Result<bool> {
        let result = sqlx::query!(
            "UPDATE sessions SET revoked = TRUE, revoked_at = NOW() WHERE refresh_token = $1 AND NOT revoked",
            token,
        )
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() > 0)
    }

    pub async fn revoke_all_for_user(&self, user_id: Uuid) -> Result<u64> {
        let result = sqlx::query!(
            "UPDATE sessions SET revoked = TRUE, revoked_at = NOW() WHERE user_id = $1 AND NOT revoked",
            user_id,
        )
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected())
    }

    pub async fn list_active_for_user(&self, user_id: Uuid) -> Result<Vec<Session>> {
        let sessions = sqlx::query_as!(
            Session,
            r#"SELECT id, user_id, refresh_token, ip_address,
                      user_agent, created_at, expires_at, revoked, revoked_at
               FROM sessions WHERE user_id = $1 AND NOT revoked AND expires_at > NOW()
               ORDER BY created_at DESC"#,
            user_id,
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(sessions)
    }
}
