use chrono::{Duration, Utc};
use jsonwebtoken::{decode, encode, Algorithm, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::AuthError;
use db::models::user::UserRole;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TokenType {
    Access,
    Refresh,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Claims {
    pub sub:        Uuid,
    pub username:   String,
    pub email:      String,
    pub role:       UserRole,
    pub token_type: TokenType,
    pub iat:        i64,
    pub exp:        i64,
    pub jti:        Uuid,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenPair {
    pub access_token:  String,
    pub refresh_token: String,
    pub expires_in:    i64,
    pub token_type:    String,
}

#[derive(Debug, Clone)]
pub struct JwtConfig {
    pub secret:                       String,
    pub access_token_expiry_minutes:  i64,
    pub refresh_token_expiry_days:    i64,
}

const DEV_FALLBACK_SECRET: &str =
    "APEX_DEV_ONLY_INSECURE_SECRET_CHANGE_IN_PRODUCTION_DO_NOT_USE_IN_PROD_00001";

impl JwtConfig {
    pub fn from_env() -> anyhow::Result<Self> {
        let secret = match std::env::var("JWT_SECRET") {
            Ok(s) if !s.trim().is_empty() => s,
            _ => {
                tracing::warn!(
                    "⚠️  JWT_SECRET not set — using insecure dev fallback. \
                     Set JWT_SECRET env var before deploying to production!"
                );
                DEV_FALLBACK_SECRET.to_string()
            }
        };
        Ok(Self {
            secret,
            access_token_expiry_minutes: std::env::var("JWT_ACCESS_TOKEN_EXPIRY_MINUTES")
                .unwrap_or_else(|_| "15".to_string())
                .parse()?,
            refresh_token_expiry_days: std::env::var("JWT_REFRESH_TOKEN_EXPIRY_DAYS")
                .unwrap_or_else(|_| "7".to_string())
                .parse()?,
        })
    }

    pub fn issue_pair(
        &self,
        user_id: Uuid,
        username: &str,
        email: &str,
        role: &UserRole,
    ) -> Result<TokenPair, AuthError> {
        let access  = self.sign_token(user_id, username, email, role, TokenType::Access, self.access_token_expiry_minutes * 60)?;
        let refresh = self.sign_token(user_id, username, email, role, TokenType::Refresh, self.refresh_token_expiry_days * 86400)?;
        Ok(TokenPair {
            access_token:  access,
            refresh_token: refresh,
            expires_in:    self.access_token_expiry_minutes * 60,
            token_type:    "Bearer".to_string(),
        })
    }

    fn sign_token(
        &self,
        user_id: Uuid,
        username: &str,
        email: &str,
        role: &UserRole,
        token_type: TokenType,
        ttl_secs: i64,
    ) -> Result<String, AuthError> {
        let now = Utc::now();
        let claims = Claims {
            sub:        user_id,
            username:   username.to_string(),
            email:      email.to_string(),
            role:       role.clone(),
            token_type,
            iat:        now.timestamp(),
            exp:        (now + Duration::seconds(ttl_secs)).timestamp(),
            jti:        Uuid::new_v4(),
        };
        encode(
            &Header::new(Algorithm::HS256),
            &claims,
            &EncodingKey::from_secret(self.secret.as_bytes()),
        )
        .map_err(|e| AuthError::Internal(e.to_string()))
    }

    pub fn validate_access(&self, token: &str) -> Result<Claims, AuthError> {
        self.validate(token, TokenType::Access)
    }

    pub fn validate_refresh(&self, token: &str) -> Result<Claims, AuthError> {
        self.validate(token, TokenType::Refresh)
    }

    fn validate(&self, token: &str, expected_type: TokenType) -> Result<Claims, AuthError> {
        let mut validation = Validation::new(Algorithm::HS256);
        validation.leeway = 5;
        let data = decode::<Claims>(
            token,
            &DecodingKey::from_secret(self.secret.as_bytes()),
            &validation,
        )
        .map_err(|e| match e.kind() {
            jsonwebtoken::errors::ErrorKind::ExpiredSignature => AuthError::TokenExpired,
            _ => AuthError::InvalidToken,
        })?;

        if data.claims.token_type != expected_type {
            return Err(AuthError::InvalidToken);
        }
        Ok(data.claims)
    }
}
