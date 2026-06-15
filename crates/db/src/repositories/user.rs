use anyhow::Result;
use sqlx::PgPool;
use uuid::Uuid;

use crate::models::user::{CreateUser, UpdateUser, User, UserRole, UserStatus};

#[derive(Clone)]
pub struct UserRepository {
    pool: PgPool,
}

impl UserRepository {
    pub fn new(pool: PgPool) -> Self { Self { pool } }

    pub async fn create(&self, req: &CreateUser, password_hash: &str) -> Result<User> {
        let user = sqlx::query_as!(
            User,
            r#"INSERT INTO users (username, email, password_hash, role)
               VALUES ($1, $2, $3, $4)
               RETURNING id, username, email, password_hash,
                         role AS "role: UserRole",
                         status AS "status: UserStatus",
                         created_at, updated_at, last_login_at,
                         mfa_enabled, mfa_secret"#,
            req.username,
            req.email,
            password_hash,
            req.role as UserRole,
        )
        .fetch_one(&self.pool)
        .await?;
        Ok(user)
    }

    pub async fn find_by_id(&self, id: Uuid) -> Result<Option<User>> {
        let user = sqlx::query_as!(
            User,
            r#"SELECT id, username, email, password_hash,
                      role AS "role: UserRole",
                      status AS "status: UserStatus",
                      created_at, updated_at, last_login_at,
                      mfa_enabled, mfa_secret
               FROM users WHERE id = $1"#,
            id,
        )
        .fetch_optional(&self.pool)
        .await?;
        Ok(user)
    }

    pub async fn find_by_email(&self, email: &str) -> Result<Option<User>> {
        let user = sqlx::query_as!(
            User,
            r#"SELECT id, username, email, password_hash,
                      role AS "role: UserRole",
                      status AS "status: UserStatus",
                      created_at, updated_at, last_login_at,
                      mfa_enabled, mfa_secret
               FROM users WHERE email = $1"#,
            email,
        )
        .fetch_optional(&self.pool)
        .await?;
        Ok(user)
    }

    pub async fn find_by_username(&self, username: &str) -> Result<Option<User>> {
        let user = sqlx::query_as!(
            User,
            r#"SELECT id, username, email, password_hash,
                      role AS "role: UserRole",
                      status AS "status: UserStatus",
                      created_at, updated_at, last_login_at,
                      mfa_enabled, mfa_secret
               FROM users WHERE username = $1"#,
            username,
        )
        .fetch_optional(&self.pool)
        .await?;
        Ok(user)
    }

    pub async fn list(&self, limit: i64, offset: i64) -> Result<Vec<User>> {
        let users = sqlx::query_as!(
            User,
            r#"SELECT id, username, email, password_hash,
                      role AS "role: UserRole",
                      status AS "status: UserStatus",
                      created_at, updated_at, last_login_at,
                      mfa_enabled, mfa_secret
               FROM users ORDER BY created_at DESC LIMIT $1 OFFSET $2"#,
            limit,
            offset,
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(users)
    }

    pub async fn update(&self, id: Uuid, req: &UpdateUser) -> Result<Option<User>> {
        let user = sqlx::query_as!(
            User,
            r#"UPDATE users
               SET email  = COALESCE($2, email),
                   role   = COALESCE($3, role),
                   status = COALESCE($4, status)
               WHERE id = $1
               RETURNING id, username, email, password_hash,
                         role AS "role: UserRole",
                         status AS "status: UserStatus",
                         created_at, updated_at, last_login_at,
                         mfa_enabled, mfa_secret"#,
            id,
            req.email,
            req.role as Option<UserRole>,
            req.status as Option<UserStatus>,
        )
        .fetch_optional(&self.pool)
        .await?;
        Ok(user)
    }

    pub async fn set_last_login(&self, id: Uuid) -> Result<()> {
        sqlx::query!("UPDATE users SET last_login_at = NOW() WHERE id = $1", id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn update_password(&self, id: Uuid, hash: &str) -> Result<()> {
        sqlx::query!("UPDATE users SET password_hash = $2 WHERE id = $1", id, hash)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn count(&self) -> Result<i64> {
        let row = sqlx::query!("SELECT COUNT(*) as cnt FROM users")
            .fetch_one(&self.pool)
            .await?;
        Ok(row.cnt.unwrap_or(0))
    }
}
