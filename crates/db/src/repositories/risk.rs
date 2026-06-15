use anyhow::Result;
use sqlx::PgPool;
use uuid::Uuid;

use crate::models::risk_rule::{CreateRiskRule, RiskRule, RiskRuleType, UpdateRiskRule};

#[derive(Clone)]
pub struct RiskRepository {
    pool: PgPool,
}

impl RiskRepository {
    pub fn new(pool: PgPool) -> Self { Self { pool } }

    pub async fn create(&self, user_id: Uuid, req: &CreateRiskRule) -> Result<RiskRule> {
        let r = sqlx::query_as!(
            RiskRule,
            r#"INSERT INTO risk_rules (rule_type, name, description, config, created_by)
               VALUES ($1,$2,$3,$4,$5)
               RETURNING id, rule_type AS "rule_type: RiskRuleType",
                         enabled, name, description, config,
                         created_by, created_at, updated_at"#,
            req.rule_type as RiskRuleType, req.name, req.description, req.config, user_id,
        )
        .fetch_one(&self.pool)
        .await?;
        Ok(r)
    }

    pub async fn list(&self) -> Result<Vec<RiskRule>> {
        let rs = sqlx::query_as!(
            RiskRule,
            r#"SELECT id, rule_type AS "rule_type: RiskRuleType",
                      enabled, name, description, config,
                      created_by, created_at, updated_at
               FROM risk_rules ORDER BY rule_type, name"#,
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rs)
    }

    pub async fn list_enabled(&self) -> Result<Vec<RiskRule>> {
        let rs = sqlx::query_as!(
            RiskRule,
            r#"SELECT id, rule_type AS "rule_type: RiskRuleType",
                      enabled, name, description, config,
                      created_by, created_at, updated_at
               FROM risk_rules WHERE enabled = TRUE ORDER BY rule_type, name"#,
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rs)
    }

    pub async fn update(&self, id: Uuid, req: &UpdateRiskRule) -> Result<Option<RiskRule>> {
        let r = sqlx::query_as!(
            RiskRule,
            r#"UPDATE risk_rules
               SET enabled     = COALESCE($2, enabled),
                   name        = COALESCE($3, name),
                   description = COALESCE($4, description),
                   config      = COALESCE($5, config)
               WHERE id = $1
               RETURNING id, rule_type AS "rule_type: RiskRuleType",
                         enabled, name, description, config,
                         created_by, created_at, updated_at"#,
            id, req.enabled, req.name, req.description, req.config,
        )
        .fetch_optional(&self.pool)
        .await?;
        Ok(r)
    }

    pub async fn delete(&self, id: Uuid) -> Result<bool> {
        let r = sqlx::query!("DELETE FROM risk_rules WHERE id = $1", id)
            .execute(&self.pool).await?;
        Ok(r.rows_affected() > 0)
    }
}
