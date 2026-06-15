use anyhow::Result;
use sqlx::PgPool;
use uuid::Uuid;

use crate::models::system_event::{CreateSystemEvent, EventCategory, EventSeverity, SystemEvent};

#[derive(Clone)]
pub struct SystemEventRepository {
    pool: PgPool,
}

impl SystemEventRepository {
    pub fn new(pool: PgPool) -> Self { Self { pool } }

    pub async fn create(&self, req: &CreateSystemEvent) -> Result<SystemEvent> {
        let e = sqlx::query_as!(
            SystemEvent,
            r#"INSERT INTO system_events (severity, category, title, message, metadata)
               VALUES ($1,$2,$3,$4,$5)
               RETURNING id,
                         severity AS "severity: EventSeverity",
                         category AS "category: EventCategory",
                         title, message, metadata, resolved, resolved_at, created_at"#,
            req.severity as EventSeverity, req.category as EventCategory,
            req.title, req.message, req.metadata,
        )
        .fetch_one(&self.pool)
        .await?;
        Ok(e)
    }

    pub async fn list(&self, limit: i64, offset: i64) -> Result<Vec<SystemEvent>> {
        let es = sqlx::query_as!(
            SystemEvent,
            r#"SELECT id,
                      severity AS "severity: EventSeverity",
                      category AS "category: EventCategory",
                      title, message, metadata, resolved, resolved_at, created_at
               FROM system_events ORDER BY created_at DESC LIMIT $1 OFFSET $2"#,
            limit, offset,
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(es)
    }

    pub async fn list_unresolved(&self) -> Result<Vec<SystemEvent>> {
        let es = sqlx::query_as!(
            SystemEvent,
            r#"SELECT id,
                      severity AS "severity: EventSeverity",
                      category AS "category: EventCategory",
                      title, message, metadata, resolved, resolved_at, created_at
               FROM system_events WHERE NOT resolved ORDER BY created_at DESC"#,
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(es)
    }

    pub async fn resolve(&self, id: Uuid) -> Result<bool> {
        let r = sqlx::query!(
            "UPDATE system_events SET resolved = TRUE, resolved_at = NOW() WHERE id = $1",
            id,
        )
        .execute(&self.pool).await?;
        Ok(r.rows_affected() > 0)
    }
}
