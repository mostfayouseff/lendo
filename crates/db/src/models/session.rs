use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Session {
    pub id:            Uuid,
    pub user_id:       Uuid,
    pub refresh_token: String,
    pub ip_address:    Option<String>,
    pub user_agent:    Option<String>,
    pub created_at:    DateTime<Utc>,
    pub expires_at:    DateTime<Utc>,
    pub revoked:       bool,
    pub revoked_at:    Option<DateTime<Utc>>,
}

#[derive(Debug, Clone)]
pub struct CreateSession {
    pub user_id:       Uuid,
    pub refresh_token: String,
    pub ip_address:    Option<String>,
    pub user_agent:    Option<String>,
    pub expires_at:    DateTime<Utc>,
}
