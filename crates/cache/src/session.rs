use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use uuid::Uuid;

use crate::client::CacheClient;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachedUser {
    pub id:       Uuid,
    pub username: String,
    pub email:    String,
    pub role:     String,
}

#[derive(Clone)]
pub struct SessionCache {
    client: CacheClient,
    ttl:    Duration,
}

impl SessionCache {
    pub fn new(client: CacheClient, ttl_secs: u64) -> Self {
        Self { client, ttl: Duration::from_secs(ttl_secs) }
    }

    fn key(session_id: &str) -> String {
        CacheClient::prefixed("session", session_id)
    }

    pub async fn store(&self, session_id: &str, user: &CachedUser) -> Result<()> {
        self.client.set_ex(&Self::key(session_id), user, self.ttl).await
    }

    pub async fn get(&self, session_id: &str) -> Result<Option<CachedUser>> {
        self.client.get(&Self::key(session_id)).await
    }

    pub async fn invalidate(&self, session_id: &str) -> Result<bool> {
        self.client.del(&Self::key(session_id)).await
    }

    pub fn invalidate_key(session_id: &str) -> String {
        Self::key(session_id)
    }
}
