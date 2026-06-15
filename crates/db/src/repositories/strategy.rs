use anyhow::Result;
use sqlx::PgPool;
use uuid::Uuid;

use crate::models::strategy::{CreateStrategy, Strategy, StrategyStatus, StrategyType, UpdateStrategy};

#[derive(Clone)]
pub struct StrategyRepository {
    pool: PgPool,
}

impl StrategyRepository {
    pub fn new(pool: PgPool) -> Self { Self { pool } }

    pub async fn create(&self, user_id: Uuid, req: &CreateStrategy) -> Result<Strategy> {
        let s = sqlx::query_as!(
            Strategy,
            r#"INSERT INTO strategies
               (user_id, name, strategy_type, min_profit_lamports, max_position_lamports,
                max_slippage_bps, max_hops, flash_loan_enabled, flash_loan_provider,
                dex_whitelist, token_whitelist, config)
               VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12)
               RETURNING id, user_id, name,
                         strategy_type  AS "strategy_type: StrategyType",
                         status         AS "status: StrategyStatus",
                         min_profit_lamports, max_position_lamports, max_slippage_bps, max_hops,
                         flash_loan_enabled, flash_loan_provider,
                         dex_whitelist, token_whitelist, config,
                         trades_executed, total_profit_lamports, created_at, updated_at"#,
            user_id, req.name, req.strategy_type as StrategyType,
            req.min_profit_lamports, req.max_position_lamports,
            req.max_slippage_bps, req.max_hops,
            req.flash_loan_enabled, req.flash_loan_provider,
            &req.dex_whitelist, &req.token_whitelist,
            req.config.clone().unwrap_or_default(),
        )
        .fetch_one(&self.pool)
        .await?;
        Ok(s)
    }

    pub async fn find_by_id(&self, id: Uuid) -> Result<Option<Strategy>> {
        let s = sqlx::query_as!(
            Strategy,
            r#"SELECT id, user_id, name,
                      strategy_type AS "strategy_type: StrategyType",
                      status        AS "status: StrategyStatus",
                      min_profit_lamports, max_position_lamports, max_slippage_bps, max_hops,
                      flash_loan_enabled, flash_loan_provider,
                      dex_whitelist, token_whitelist, config,
                      trades_executed, total_profit_lamports, created_at, updated_at
               FROM strategies WHERE id = $1"#, id,
        )
        .fetch_optional(&self.pool)
        .await?;
        Ok(s)
    }

    pub async fn list_by_user(&self, user_id: Uuid) -> Result<Vec<Strategy>> {
        let ss = sqlx::query_as!(
            Strategy,
            r#"SELECT id, user_id, name,
                      strategy_type AS "strategy_type: StrategyType",
                      status        AS "status: StrategyStatus",
                      min_profit_lamports, max_position_lamports, max_slippage_bps, max_hops,
                      flash_loan_enabled, flash_loan_provider,
                      dex_whitelist, token_whitelist, config,
                      trades_executed, total_profit_lamports, created_at, updated_at
               FROM strategies WHERE user_id = $1 ORDER BY created_at DESC"#, user_id,
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(ss)
    }

    pub async fn list_active(&self) -> Result<Vec<Strategy>> {
        let ss = sqlx::query_as!(
            Strategy,
            r#"SELECT id, user_id, name,
                      strategy_type AS "strategy_type: StrategyType",
                      status        AS "status: StrategyStatus",
                      min_profit_lamports, max_position_lamports, max_slippage_bps, max_hops,
                      flash_loan_enabled, flash_loan_provider,
                      dex_whitelist, token_whitelist, config,
                      trades_executed, total_profit_lamports, created_at, updated_at
               FROM strategies WHERE status = 'active' ORDER BY created_at DESC"#,
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(ss)
    }

    pub async fn update(&self, id: Uuid, req: &UpdateStrategy) -> Result<Option<Strategy>> {
        let s = sqlx::query_as!(
            Strategy,
            r#"UPDATE strategies
               SET name                   = COALESCE($2, name),
                   status                 = COALESCE($3, status),
                   min_profit_lamports    = COALESCE($4, min_profit_lamports),
                   max_position_lamports  = COALESCE($5, max_position_lamports),
                   max_slippage_bps       = COALESCE($6, max_slippage_bps),
                   max_hops               = COALESCE($7, max_hops),
                   flash_loan_enabled     = COALESCE($8, flash_loan_enabled),
                   flash_loan_provider    = COALESCE($9, flash_loan_provider),
                   dex_whitelist          = COALESCE($10, dex_whitelist),
                   token_whitelist        = COALESCE($11, token_whitelist),
                   config                 = COALESCE($12, config)
               WHERE id = $1
               RETURNING id, user_id, name,
                         strategy_type AS "strategy_type: StrategyType",
                         status        AS "status: StrategyStatus",
                         min_profit_lamports, max_position_lamports, max_slippage_bps, max_hops,
                         flash_loan_enabled, flash_loan_provider,
                         dex_whitelist, token_whitelist, config,
                         trades_executed, total_profit_lamports, created_at, updated_at"#,
            id, req.name, req.status as Option<StrategyStatus>,
            req.min_profit_lamports, req.max_position_lamports,
            req.max_slippage_bps, req.max_hops,
            req.flash_loan_enabled, req.flash_loan_provider,
            req.dex_whitelist.as_deref(), req.token_whitelist.as_deref(), req.config,
        )
        .fetch_optional(&self.pool)
        .await?;
        Ok(s)
    }

    pub async fn record_trade(&self, id: Uuid, profit_lamports: i64) -> Result<()> {
        sqlx::query!(
            "UPDATE strategies SET trades_executed = trades_executed + 1, \
             total_profit_lamports = total_profit_lamports + $2 WHERE id = $1",
            id, profit_lamports,
        )
        .execute(&self.pool).await?;
        Ok(())
    }

    pub async fn delete(&self, id: Uuid) -> Result<bool> {
        let r = sqlx::query!("DELETE FROM strategies WHERE id = $1", id)
            .execute(&self.pool).await?;
        Ok(r.rows_affected() > 0)
    }
}
