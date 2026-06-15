use anyhow::Result;
use std::time::Duration;

use crate::client::CacheClient;

#[derive(Clone)]
pub struct RateLimiter {
    cache:           CacheClient,
    max_requests:    i64,
    window_duration: Duration,
}

impl RateLimiter {
    pub fn new(cache: CacheClient, max_requests: i64, window: Duration) -> Self {
        Self { cache, max_requests: max_requests, window_duration: window }
    }

    pub async fn check_and_increment(&self, identifier: &str) -> Result<RateLimitResult> {
        let key = CacheClient::prefixed("rl", identifier);
        let current = self.cache.incr_ex(&key, self.window_duration).await?;
        let remaining = (self.max_requests - current).max(0);
        Ok(RateLimitResult {
            allowed:   current <= self.max_requests,
            current:   current as u64,
            limit:     self.max_requests as u64,
            remaining: remaining as u64,
            reset_in:  self.window_duration,
        })
    }
}

#[derive(Debug, Clone)]
pub struct RateLimitResult {
    pub allowed:   bool,
    pub current:   u64,
    pub limit:     u64,
    pub remaining: u64,
    pub reset_in:  Duration,
}
