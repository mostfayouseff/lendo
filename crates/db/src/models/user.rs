use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::Type;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Type, Serialize, Deserialize)]
#[sqlx(type_name = "user_role", rename_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub enum UserRole {
    Admin,
    Trader,
    Viewer,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Type, Serialize, Deserialize)]
#[sqlx(type_name = "user_status", rename_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub enum UserStatus {
    Active,
    Suspended,
    Pending,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct User {
    pub id:            Uuid,
    pub username:      String,
    pub email:         String,
    #[serde(skip_serializing)]
    pub password_hash: String,
    pub role:          UserRole,
    pub status:        UserStatus,
    pub created_at:    DateTime<Utc>,
    pub updated_at:    DateTime<Utc>,
    pub last_login_at: Option<DateTime<Utc>>,
    pub mfa_enabled:   bool,
    #[serde(skip)]
    pub mfa_secret:    Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PublicUser {
    pub id:            Uuid,
    pub username:      String,
    pub email:         String,
    pub role:          UserRole,
    pub status:        UserStatus,
    pub created_at:    DateTime<Utc>,
    pub last_login_at: Option<DateTime<Utc>>,
    pub mfa_enabled:   bool,
}

impl From<User> for PublicUser {
    fn from(u: User) -> Self {
        Self {
            id:            u.id,
            username:      u.username,
            email:         u.email,
            role:          u.role,
            status:        u.status,
            created_at:    u.created_at,
            last_login_at: u.last_login_at,
            mfa_enabled:   u.mfa_enabled,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateUser {
    pub username: String,
    pub email:    String,
    pub password: String,
    pub role:     UserRole,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateUser {
    pub email:  Option<String>,
    pub role:   Option<UserRole>,
    pub status: Option<UserStatus>,
}
