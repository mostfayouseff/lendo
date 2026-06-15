use anyhow::Result;
use sqlx::PgPool;
use uuid::Uuid;

use crate::models::trade::{CreateTrade, Trade, TradeStatus, TradeSummary};

#[derive(Clone)]
pub struct TradeRepository {
    pool: PgPool,
}

impl TradeRepository {
    pub fn new(pool: PgPool) -> Self { Self { pool } }

    pub async fn create(&self, req: &CreateTrade) -> Result<Trade> {
        let t = sqlx::query_as!(
            Trade,
            r#"INSERT INTO trades
               (opportunity_id, strategy_id, wallet_id, input_mint, output_mint,
                input_amount_lamports, expected_profit_lamports, fee_lamports,
                jito_tip_lamports, flash_loan_fee_lamports, hop_count, dex_path)
               VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12)
               RETURNING id, opportunity_id, strategy_id, wallet_id,
                         status AS "status: TradeStatus",
                         signature, input_mint, output_mint,
                         input_amount_lamports, output_amount_lamports,
                         expected_profit_lamports, actual_profit_lamports,
                         fee_lamports, jito_tip_lamports, flash_loan_fee_lamports,
                         slippage_bps, hop_count, dex_path, simulation_passed,
                         error_message, slot, block_time, created_at, confirmed_at, updated_at"#,
            req.opportunity_id, req.strategy_id, req.wallet_id,
            req.input_mint, req.output_mint,
            req.input_amount_lamports, req.expected_profit_lamports,
            req.fee_lamports, req.jito_tip_lamports, req.flash_loan_fee_lamports,
            req.hop_count, req.dex_path,
        )
        .fetch_one(&self.pool)
        .await?;
        Ok(t)
    }

    pub async fn find_by_id(&self, id: Uuid) -> Result<Option<Trade>> {
        let t = sqlx::query_as!(
            Trade,
            r#"SELECT id, opportunity_id, strategy_id, wallet_id,
                      status AS "status: TradeStatus",
                      signature, input_mint, output_mint,
                      input_amount_lamports, output_amount_lamports,
                      expected_profit_lamports, actual_profit_lamports,
                      fee_lamports, jito_tip_lamports, flash_loan_fee_lamports,
                      slippage_bps, hop_count, dex_path, simulation_passed,
                      error_message, slot, block_time, created_at, confirmed_at, updated_at
               FROM trades WHERE id = $1"#, id,
        )
        .fetch_optional(&self.pool)
        .await?;
        Ok(t)
    }

    pub async fn list(&self, limit: i64, offset: i64) -> Result<Vec<Trade>> {
        let ts = sqlx::query_as!(
            Trade,
            r#"SELECT id, opportunity_id, strategy_id, wallet_id,
                      status AS "status: TradeStatus",
                      signature, input_mint, output_mint,
                      input_amount_lamports, output_amount_lamports,
                      expected_profit_lamports, actual_profit_lamports,
                      fee_lamports, jito_tip_lamports, flash_loan_fee_lamports,
                      slippage_bps, hop_count, dex_path, simulation_passed,
                      error_message, slot, block_time, created_at, confirmed_at, updated_at
               FROM trades ORDER BY created_at DESC LIMIT $1 OFFSET $2"#,
            limit, offset,
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(ts)
    }

    pub async fn confirm(
        &self, id: Uuid, signature: &str, slot: i64,
        actual_profit: i64, output_amount: i64,
    ) -> Result<()> {
        sqlx::query!(
            r#"UPDATE trades SET status = 'confirmed', signature = $2, slot = $3,
               actual_profit_lamports = $4, output_amount_lamports = $5,
               confirmed_at = NOW()
               WHERE id = $1"#,
            id, signature, slot, actual_profit, output_amount,
        )
        .execute(&self.pool).await?;
        Ok(())
    }

    pub async fn fail(&self, id: Uuid, error: &str) -> Result<()> {
        sqlx::query!(
            "UPDATE trades SET status = 'failed', error_message = $2 WHERE id = $1",
            id, error,
        )
        .execute(&self.pool).await?;
        Ok(())
    }

    pub async fn update_status(&self, id: Uuid, status: TradeStatus) -> Result<()> {
        sqlx::query!(
            "UPDATE trades SET status = $2 WHERE id = $1",
            id, status as TradeStatus,
        )
        .execute(&self.pool).await?;
        Ok(())
    }

    pub async fn summary(&self) -> Result<TradeSummary> {
        let row = sqlx::query!(
            r#"SELECT
                COUNT(*)                                                                           AS total_trades,
                COUNT(*) FILTER (WHERE status = 'confirmed')                                      AS confirmed_trades,
                COUNT(*) FILTER (WHERE status = 'failed')                                         AS failed_trades,
                COALESCE(SUM(actual_profit_lamports) FILTER (WHERE status = 'confirmed'), 0)::bigint AS total_profit
               FROM trades"#,
        )
        .fetch_one(&self.pool)
        .await?;

        let total    = row.total_trades.unwrap_or(0);
        let conf     = row.confirmed_trades.unwrap_or(0);
        let failed   = row.failed_trades.unwrap_or(0);
        let profit   = row.total_profit.unwrap_or(0);
        let win_rate    = if total > 0 { conf as f64 / total as f64 } else { 0.0 };
        let avg_profit  = if conf > 0 { profit as f64 / conf as f64 } else { 0.0 };

        Ok(TradeSummary {
            total_trades:          total,
            confirmed_trades:      conf,
            failed_trades:         failed,
            total_profit_lamports: profit,
            total_profit_sol:      profit as f64 / 1e9,
            win_rate,
            avg_profit_lamports:   avg_profit,
        })
    }
}
