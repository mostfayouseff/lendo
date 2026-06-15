use anyhow::Result;
use redis::{aio::ConnectionManager, AsyncCommands, Client};
use serde::{de::DeserializeOwned, Serialize};
use std::time::Duration;
use tracing::{info, warn};

/// Redis cache client. When Redis is unavailable all operations silently no-op
/// so the API server can start and serve requests without a cache backend.
#[derive(Clone)]
pub struct CacheClient {
    manager: Option<ConnectionManager>,
}

impl CacheClient {
    /// Connect to Redis. Returns an operational client on success, or a no-op
    /// client that logs a warning and continues if Redis is unreachable.
    pub async fn connect(url: &str) -> Result<Self> {
        info!("Connecting to Redis at {url}");
        match Client::open(url) {
            Ok(client) => match ConnectionManager::new(client).await {
                Ok(manager) => {
                    info!("Redis connection established");
                    Ok(Self { manager: Some(manager) })
                }
                Err(e) => {
                    warn!("Redis unavailable ({e}) — running without cache. Rate limiting and session persistence disabled.");
                    Ok(Self { manager: None })
                }
            },
            Err(e) => {
                warn!("Invalid Redis URL ({e}) — running without cache.");
                Ok(Self { manager: None })
            }
        }
    }

    pub fn is_connected(&self) -> bool {
        self.manager.is_some()
    }

    pub async fn set_ex<T: Serialize>(&self, key: &str, value: &T, ttl: Duration) -> Result<()> {
        let Some(ref mgr) = self.manager else { return Ok(()) };
        let mut conn = mgr.clone();
        let json = serde_json::to_string(value)?;
        conn.set_ex::<_, _, ()>(key, json, ttl.as_secs()).await?;
        Ok(())
    }

    pub async fn get<T: DeserializeOwned>(&self, key: &str) -> Result<Option<T>> {
        let Some(ref mgr) = self.manager else { return Ok(None) };
        let mut conn = mgr.clone();
        let raw: Option<String> = conn.get(key).await?;
        match raw {
            Some(s) => Ok(Some(serde_json::from_str(&s)?)),
            None    => Ok(None),
        }
    }

    pub async fn del(&self, key: &str) -> Result<bool> {
        let Some(ref mgr) = self.manager else { return Ok(false) };
        let mut conn = mgr.clone();
        let n: i64 = conn.del(key).await?;
        Ok(n > 0)
    }

    pub async fn exists(&self, key: &str) -> Result<bool> {
        let Some(ref mgr) = self.manager else { return Ok(false) };
        let mut conn = mgr.clone();
        let n: i64 = conn.exists(key).await?;
        Ok(n > 0)
    }

    pub async fn incr_ex(&self, key: &str, ttl: Duration) -> Result<i64> {
        let Some(ref mgr) = self.manager else { return Ok(0) };
        let mut conn = mgr.clone();
        let (n, _): (i64, bool) = redis::pipe()
            .atomic()
            .incr(key, 1)
            .expire(key, ttl.as_secs() as i64)
            .query_async(&mut conn)
            .await?;
        Ok(n)
    }

    pub async fn set_nx<T: Serialize>(&self, key: &str, value: &T, ttl: Duration) -> Result<bool> {
        let Some(ref mgr) = self.manager else { return Ok(true) };
        let mut conn = mgr.clone();
        let json = serde_json::to_string(value)?;
        let ok: bool = conn.set_nx(key, &json).await?;
        if ok {
            conn.expire::<_, ()>(key, ttl.as_secs() as i64).await?;
        }
        Ok(ok)
    }

    pub async fn health_check(&self) -> Result<()> {
        let Some(ref mgr) = self.manager else {
            return Err(anyhow::anyhow!("Redis not connected"));
        };
        let mut conn = mgr.clone();
        redis::cmd("PING").query_async::<_, String>(&mut conn).await?;
        Ok(())
    }

    pub fn prefixed(prefix: &str, key: &str) -> String {
        format!("{prefix}:{key}")
    }
}
