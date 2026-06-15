use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AuthError {
    #[error("Invalid credentials")]
    InvalidCredentials,
    #[error("Token expired")]
    TokenExpired,
    #[error("Invalid token")]
    InvalidToken,
    #[error("Token not found")]
    TokenNotFound,
    #[error("Insufficient permissions: required role '{0}'")]
    InsufficientPermissions(String),
    #[error("Account suspended")]
    AccountSuspended,
    #[error("Account pending activation")]
    AccountPending,
    #[error("Internal error: {0}")]
    Internal(String),
}

impl IntoResponse for AuthError {
    fn into_response(self) -> Response {
        let (status, code) = match &self {
            AuthError::InvalidCredentials     => (StatusCode::UNAUTHORIZED,  "INVALID_CREDENTIALS"),
            AuthError::TokenExpired           => (StatusCode::UNAUTHORIZED,  "TOKEN_EXPIRED"),
            AuthError::InvalidToken           => (StatusCode::UNAUTHORIZED,  "INVALID_TOKEN"),
            AuthError::TokenNotFound          => (StatusCode::UNAUTHORIZED,  "TOKEN_NOT_FOUND"),
            AuthError::InsufficientPermissions(_) => (StatusCode::FORBIDDEN, "FORBIDDEN"),
            AuthError::AccountSuspended       => (StatusCode::FORBIDDEN,     "ACCOUNT_SUSPENDED"),
            AuthError::AccountPending         => (StatusCode::FORBIDDEN,     "ACCOUNT_PENDING"),
            AuthError::Internal(_)            => (StatusCode::INTERNAL_SERVER_ERROR, "INTERNAL_ERROR"),
        };
        (status, Json(json!({ "error": { "code": code, "message": self.to_string() } }))).into_response()
    }
}
