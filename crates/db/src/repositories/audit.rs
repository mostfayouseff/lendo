use anyhow::Result;
use sqlx::PgPool;
use uuid::Uuid;

use crate::models::audit_log::{AuditAction, AuditLog, CreateAuditLog};

#[derive(Clone)]
pub struct AuditRepository {
    pool: PgPool,
}

impl AuditRepository {
    pub fn new(pool: PgPool) -> Self { Self { pool } }

    pub async fn create(&self, req: &CreateAuditLog) -> Result<AuditLog> {
        let log = sqlx::query_as!(
            AuditLog,
            r#"INSERT INTO audit_logs
               (user_id, action, entity_type, entity_id, old_value, new_value,
                ip_address, user_agent, metadata)
               VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9)
               RETURNING id, user_id, action AS "action: AuditAction",
                         entity_type, entity_id, old_value, new_value,
                         ip_address, user_agent, metadata, created_at"#,
            req.user_id, req.action as AuditAction, req.entity_type,
            req.entity_id, req.old_value, req.new_value,
            req.ip_address, req.user_agent, req.metadata,
        )
        .fetch_one(&self.pool)
        .await?;
        Ok(log)
    }

    pub async fn list(&self, limit: i64, offset: i64) -> Result<Vec<AuditLog>> {
        let logs = sqlx::query_as!(
            AuditLog,
            r#"SELECT id, user_id, action AS "action: AuditAction",
                      entity_type, entity_id, old_value, new_value,
                      ip_address, user_agent, metadata, created_at
               FROM audit_logs ORDER BY created_at DESC LIMIT $1 OFFSET $2"#,
            limit, offset,
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(logs)
    }

    pub async fn list_by_user(&self, user_id: Uuid, limit: i64) -> Result<Vec<AuditLog>> {
        let logs = sqlx::query_as!(
            AuditLog,
            r#"SELECT id, user_id, action AS "action: AuditAction",
                      entity_type, entity_id, old_value, new_value,
                      ip_address, user_agent, metadata, created_at
               FROM audit_logs WHERE user_id = $1 ORDER BY created_at DESC LIMIT $2"#,
            user_id, limit,
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(logs)
    }
}
