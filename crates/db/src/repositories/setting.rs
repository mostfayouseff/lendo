use anyhow::Result;
use sqlx::PgPool;
use uuid::Uuid;

use crate::models::setting::Setting;

#[derive(Clone)]
pub struct SettingRepository {
    pool: PgPool,
}

impl SettingRepository {
    pub fn new(pool: PgPool) -> Self { Self { pool } }

    pub async fn get(&self, key: &str) -> Result<Option<Setting>> {
        let s = sqlx::query_as!(
            Setting,
            "SELECT key, value, description, updated_by, updated_at FROM settings WHERE key = $1",
            key,
        )
        .fetch_optional(&self.pool)
        .await?;
        Ok(s)
    }

    pub async fn get_all(&self) -> Result<Vec<Setting>> {
        let ss = sqlx::query_as!(
            Setting,
            "SELECT key, value, description, updated_by, updated_at FROM settings ORDER BY key",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(ss)
    }

    pub async fn set(&self, key: &str, value: serde_json::Value, updated_by: Option<Uuid>) -> Result<Setting> {
        let s = sqlx::query_as!(
            Setting,
            r#"INSERT INTO settings (key, value, updated_by)
               VALUES ($1, $2, $3)
               ON CONFLICT (key) DO UPDATE
               SET value = EXCLUDED.value, updated_by = EXCLUDED.updated_by, updated_at = NOW()
               RETURNING key, value, description, updated_by, updated_at"#,
            key, value, updated_by,
        )
        .fetch_one(&self.pool)
        .await?;
        Ok(s)
    }

    pub async fn get_json<T: serde::de::DeserializeOwned>(&self, key: &str) -> Result<Option<T>> {
        match self.get(key).await? {
            Some(s) => Ok(Some(serde_json::from_value(s.value)?)),
            None => Ok(None),
        }
    }

    pub async fn get_bool(&self, key: &str, default: bool) -> Result<bool> {
        self.get_json::<bool>(key).await.map(|v| v.unwrap_or(default))
    }

    pub async fn get_i64(&self, key: &str, default: i64) -> Result<i64> {
        self.get_json::<i64>(key).await.map(|v| v.unwrap_or(default))
    }

    pub async fn get_string(&self, key: &str, default: &str) -> Result<String> {
        self.get_json::<String>(key).await.map(|v| v.unwrap_or_else(|| default.to_string()))
    }
}
