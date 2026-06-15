use anyhow::Result;
use dotenvy::dotenv;
use tracing::info;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

mod error;
mod handlers;
mod middleware;
mod router;
mod state;
mod ws;

#[tokio::main]
async fn main() -> Result<()> {
    dotenv().ok();

    tracing_subscriber::registry()
        .with(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .with(tracing_subscriber::fmt::layer())
        .init();

    info!("Apex-MEV API Server starting");

    let app_state = state::AppState::from_env().await?;
    app_state.db.run_migrations().await?;
    app_state.seed_admin().await?;

    let router = router::build(app_state.clone());

    let host = std::env::var("API_HOST").unwrap_or_else(|_| "0.0.0.0".to_string());
    let port = std::env::var("API_PORT")
        .unwrap_or_else(|_| "5000".to_string())
        .parse::<u16>()?;
    let addr = format!("{host}:{port}");

    info!(addr = %addr, "API server listening");

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, router).await?;
    Ok(())
}
