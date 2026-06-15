use anyhow::{Context, Result};
use std::sync::Arc;
use tokio::sync::broadcast;
use tracing::info;

use auth::{password::hash_password, jwt::JwtConfig};
use cache::{CacheClient, RateLimiter, SessionCache};
use db::{
    models::user::{CreateUser, UpdateUser, UserRole, UserStatus},
    repositories::{
        AuditRepository, OpportunityRepository, RiskRepository, SessionRepository,
        SettingRepository, StrategyRepository, SystemEventRepository, TokenRepository,
        TradeRepository, UserRepository, WalletRepository,
    },
    Database,
};

use crate::ws::WsEvent;

#[derive(Clone)]
pub struct AppState {
    pub db:             Database,
    pub cache:          CacheClient,
    pub jwt:            JwtConfig,
    pub session_cache:  SessionCache,
    pub rate_limiter:   RateLimiter,
    pub ws_tx:          broadcast::Sender<WsEvent>,

    pub users:       UserRepository,
    pub sessions:    SessionRepository,
    pub wallets:     WalletRepository,
    pub tokens:      TokenRepository,
    pub strategies:  StrategyRepository,
    pub trades:      TradeRepository,
    pub opps:        OpportunityRepository,
    pub settings:    SettingRepository,
    pub audit:       AuditRepository,
    pub events:      SystemEventRepository,
    pub risk_rules:  RiskRepository,
}

impl AppState {
    pub async fn from_env() -> Result<Arc<Self>> {
        let db_url = std::env::var("DATABASE_URL")
            .context("DATABASE_URL required")?;
        let db_max = std::env::var("DATABASE_MAX_CONNECTIONS")
            .unwrap_or_else(|_| "20".to_string())
            .parse::<u32>()?;
        let redis_url = std::env::var("REDIS_URL")
            .unwrap_or_else(|_| "redis://localhost:6379".to_string());

        let db    = Database::connect(&db_url, db_max).await?;
        let cache = CacheClient::connect(&redis_url).await?;
        let jwt   = JwtConfig::from_env()?;

        let session_cache = SessionCache::new(cache.clone(), 900);
        let rate_limiter  = RateLimiter::new(cache.clone(), 100, std::time::Duration::from_secs(60));

        let (ws_tx, _) = broadcast::channel(1024);

        let pool = db.pool.clone();

        let state = Self {
            db:            db.clone(),
            cache:         cache.clone(),
            jwt,
            session_cache,
            rate_limiter,
            ws_tx,
            users:         UserRepository::new(pool.clone()),
            sessions:      SessionRepository::new(pool.clone()),
            wallets:       WalletRepository::new(pool.clone()),
            tokens:        TokenRepository::new(pool.clone()),
            strategies:    StrategyRepository::new(pool.clone()),
            trades:        TradeRepository::new(pool.clone()),
            opps:          OpportunityRepository::new(pool.clone()),
            settings:      SettingRepository::new(pool.clone()),
            audit:         AuditRepository::new(pool.clone()),
            events:        SystemEventRepository::new(pool.clone()),
            risk_rules:    RiskRepository::new(pool.clone()),
        };

        info!("AppState initialized");
        Ok(Arc::new(state))
    }

    pub async fn seed_admin(self: &Arc<Self>) -> Result<()> {
        let email = std::env::var("ADMIN_EMAIL")
            .unwrap_or_else(|_| "admin@apex.local".to_string());

        if self.users.find_by_email(&email).await?.is_some() {
            return Ok(());
        }

        let username = std::env::var("ADMIN_USERNAME").unwrap_or_else(|_| "admin".to_string());
        let password = std::env::var("ADMIN_PASSWORD").unwrap_or_else(|_| "changeme".to_string());
        let hash     = hash_password(&password)?;

        let user = self.users.create(
            &CreateUser { username, email, password: password.clone(), role: UserRole::Admin },
            &hash,
        ).await?;

        self.users.update(user.id, &UpdateUser {
            email:  None,
            role:   None,
            status: Some(UserStatus::Active),
        }).await?;

        info!("Admin user seeded");
        Ok(())
    }

    pub fn broadcast(&self, event: WsEvent) {
        let _ = self.ws_tx.send(event);
    }
}
