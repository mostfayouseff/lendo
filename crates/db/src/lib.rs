pub mod models;
pub mod repositories;

use anyhow::Result;
use sqlx::{postgres::PgPoolOptions, PgPool};
use tracing::info;

pub use sqlx;

#[derive(Clone)]
pub struct Database {
    pub pool: PgPool,
}

impl Database {
    pub async fn connect(url: &str, max_connections: u32) -> Result<Self> {
        info!(url = %url.split('@').last().unwrap_or("***"), max = max_connections, "Connecting to PostgreSQL");
        let pool = PgPoolOptions::new()
            .max_connections(max_connections)
            .min_connections(1)
            .acquire_timeout(std::time::Duration::from_secs(30))
            .connect(url)
            .await?;
        info!("PostgreSQL connection pool established");
        Ok(Self { pool })
    }

    pub async fn run_migrations(&self) -> Result<()> {
        info!("Running database migrations");
        sqlx::migrate!("../../database/migrations")
            .run(&self.pool)
            .await?;
        info!("Database migrations complete");
        Ok(())
    }

    pub async fn health_check(&self) -> Result<()> {
        sqlx::query("SELECT 1").fetch_one(&self.pool).await?;
        Ok(())
    }
}
