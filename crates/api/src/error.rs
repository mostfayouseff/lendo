use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ApiError {
    #[error("{0}")]
    Auth(#[from] auth::AuthError),
    #[error("Not found: {0}")]
    NotFound(String),
    #[error("Bad request: {0}")]
    BadRequest(String),
    #[error("Conflict: {0}")]
    Conflict(String),
    #[error("Forbidden: {0}")]
    Forbidden(String),
    #[error("Rate limit exceeded")]
    RateLimited,
    #[error("Internal error: {0}")]
    Internal(#[from] anyhow::Error),
    #[error("Database error: {0}")]
    Database(#[from] sqlx::Error),
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        match self {
            ApiError::Auth(e) => e.into_response(),
            ApiError::NotFound(m) => (
                StatusCode::NOT_FOUND,
                Json(json!({ "error": { "code": "NOT_FOUND", "message": m } })),
            ).into_response(),
            ApiError::BadRequest(m) => (
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": { "code": "BAD_REQUEST", "message": m } })),
            ).into_response(),
            ApiError::Conflict(m) => (
                StatusCode::CONFLICT,
                Json(json!({ "error": { "code": "CONFLICT", "message": m } })),
            ).into_response(),
            ApiError::Forbidden(m) => (
                StatusCode::FORBIDDEN,
                Json(json!({ "error": { "code": "FORBIDDEN", "message": m } })),
            ).into_response(),
            ApiError::RateLimited => (
                StatusCode::TOO_MANY_REQUESTS,
                Json(json!({ "error": { "code": "RATE_LIMITED", "message": "Too many requests" } })),
            ).into_response(),
            ApiError::Internal(e) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": { "code": "INTERNAL", "message": e.to_string() } })),
            ).into_response(),
            ApiError::Database(e) => {
                tracing::error!("DB error: {e}");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({ "error": { "code": "DB_ERROR", "message": "Database error" } })),
                ).into_response()
            }
        }
    }
}

pub type ApiResult<T> = Result<T, ApiError>;
