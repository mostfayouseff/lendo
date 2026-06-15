use anyhow::Result;
use rust_decimal::Decimal;
use sqlx::PgPool;
use uuid::Uuid;

use crate::models::opportunity::{CreateOpportunity, Opportunity, OpportunityStatus};

#[derive(Clone)]
pub struct OpportunityRepository {
    pool: PgPool,
}

impl OpportunityRepository {
    pub fn new(pool: PgPool) -> Self { Self { pool } }

    pub async fn create(&self, req: &CreateOpportunity) -> Result<Opportunity> {
        let gnn = req.gnn_confidence
            .and_then(|v| Decimal::from_f64_retain(v));
        let o = sqlx::query_as!(
            Opportunity,
            r#"INSERT INTO opportunities
               (strategy_id, path, dex_path, input_mint, output_mint,
                input_amount_lamports, estimated_profit_lamports, hop_count, gnn_confidence)
               VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9)
               RETURNING id, strategy_id,
                         status AS "status: OpportunityStatus",
                         path, dex_path, input_mint, output_mint,
                         input_amount_lamports, estimated_profit_lamports,
                         estimated_profit_usd, price_impact_pct, hop_count,
                         gnn_confidence, skip_reason, detected_at, executed_at"#,
            req.strategy_id, &req.path, req.dex_path,
            req.input_mint, req.output_mint,
            req.input_amount_lamports, req.estimated_profit_lamports,
            req.hop_count,
            gnn,
        )
        .fetch_one(&self.pool)
        .await?;
        Ok(o)
    }

    pub async fn list(&self, limit: i64, offset: i64) -> Result<Vec<Opportunity>> {
        let os = sqlx::query_as!(
            Opportunity,
            r#"SELECT id, strategy_id,
                      status AS "status: OpportunityStatus",
                      path, dex_path, input_mint, output_mint,
                      input_amount_lamports, estimated_profit_lamports,
                      estimated_profit_usd, price_impact_pct, hop_count,
                      gnn_confidence, skip_reason, detected_at, executed_at
               FROM opportunities ORDER BY detected_at DESC LIMIT $1 OFFSET $2"#,
            limit, offset,
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(os)
    }

    pub async fn list_recent(&self, hours: i64) -> Result<Vec<Opportunity>> {
        let os = sqlx::query_as!(
            Opportunity,
            r#"SELECT id, strategy_id,
                      status AS "status: OpportunityStatus",
                      path, dex_path, input_mint, output_mint,
                      input_amount_lamports, estimated_profit_lamports,
                      estimated_profit_usd, price_impact_pct, hop_count,
                      gnn_confidence, skip_reason, detected_at, executed_at
               FROM opportunities
               WHERE detected_at > NOW() - ($1 || ' hours')::INTERVAL
               ORDER BY detected_at DESC"#,
            hours.to_string(),
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(os)
    }

    pub async fn update_status(&self, id: Uuid, status: OpportunityStatus) -> Result<()> {
        sqlx::query!(
            "UPDATE opportunities SET status = $2 WHERE id = $1",
            id, status as OpportunityStatus,
        )
        .execute(&self.pool).await?;
        Ok(())
    }

    pub async fn count_today(&self) -> Result<i64> {
        let row = sqlx::query!(
            "SELECT COUNT(*) AS cnt FROM opportunities WHERE detected_at > NOW() - INTERVAL '24 hours'"
        )
        .fetch_one(&self.pool).await?;
        Ok(row.cnt.unwrap_or(0))
    }
}
