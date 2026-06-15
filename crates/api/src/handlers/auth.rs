use axum::{extract::State, Json};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use auth::{password::{hash_password, verify_password}, jwt::TokenPair};
use db::models::{
    session::CreateSession,
    user::{CreateUser, UserRole, UserStatus},
};

use crate::{error::{ApiError, ApiResult}, state::AppState};

#[derive(Debug, Deserialize)]
pub struct LoginRequest {
    pub email:    String,
    pub password: String,
}

#[derive(Debug, Serialize)]
pub struct LoginResponse {
    pub tokens: TokenPair,
    pub user: serde_json::Value,
}

pub async fn login(
    State(state): State<Arc<AppState>>,
    Json(req): Json<LoginRequest>,
) -> ApiResult<Json<LoginResponse>> {
    let user = state.users.find_by_email(&req.email).await?
        .ok_or_else(|| ApiError::Auth(auth::AuthError::InvalidCredentials))?;

    if !verify_password(&req.password, &user.password_hash)
        .map_err(|e| ApiError::Auth(e))? {
        return Err(ApiError::Auth(auth::AuthError::InvalidCredentials));
    }

    match user.status {
        UserStatus::Suspended => return Err(ApiError::Auth(auth::AuthError::AccountSuspended)),
        UserStatus::Pending   => return Err(ApiError::Auth(auth::AuthError::AccountPending)),
        UserStatus::Active    => {}
    }

    let tokens = state.jwt.issue_pair(user.id, &user.username, &user.email, &user.role)
        .map_err(ApiError::Auth)?;

    let expires_at = chrono::Utc::now() + chrono::Duration::days(
        std::env::var("JWT_REFRESH_TOKEN_EXPIRY_DAYS")
            .unwrap_or_else(|_| "7".to_string())
            .parse::<i64>()
            .unwrap_or(7)
    );

    state.sessions.create(&CreateSession {
        user_id:       user.id,
        refresh_token: tokens.refresh_token.clone(),
        ip_address:    None,
        user_agent:    None,
        expires_at,
    }).await?;

    state.users.set_last_login(user.id).await?;

    Ok(Json(LoginResponse {
        tokens,
        user: serde_json::json!({
            "id": user.id, "username": user.username,
            "email": user.email, "role": user.role
        }),
    }))
}

#[derive(Debug, Deserialize)]
pub struct RefreshRequest {
    pub refresh_token: String,
}

pub async fn refresh(
    State(state): State<Arc<AppState>>,
    Json(req): Json<RefreshRequest>,
) -> ApiResult<Json<TokenPair>> {
    let claims = state.jwt.validate_refresh(&req.refresh_token)
        .map_err(ApiError::Auth)?;

    let session = state.sessions.find_by_token(&req.refresh_token).await?
        .ok_or_else(|| ApiError::Auth(auth::AuthError::TokenNotFound))?;

    let user = state.users.find_by_id(session.user_id).await?
        .ok_or_else(|| ApiError::NotFound("User not found".into()))?;

    state.sessions.revoke(&req.refresh_token).await?;

    let tokens = state.jwt.issue_pair(user.id, &user.username, &user.email, &user.role)
        .map_err(ApiError::Auth)?;

    let expires_at = chrono::Utc::now() + chrono::Duration::days(7);
    state.sessions.create(&CreateSession {
        user_id:       user.id,
        refresh_token: tokens.refresh_token.clone(),
        ip_address:    None,
        user_agent:    None,
        expires_at,
    }).await?;

    let _ = claims;
    Ok(Json(tokens))
}

pub async fn logout(
    State(state): State<Arc<AppState>>,
    Json(req): Json<RefreshRequest>,
) -> ApiResult<Json<serde_json::Value>> {
    state.sessions.revoke(&req.refresh_token).await?;
    Ok(Json(serde_json::json!({ "message": "Logged out" })))
}

#[derive(Debug, Deserialize)]
pub struct RegisterRequest {
    pub username: String,
    pub email:    String,
    pub password: String,
}

pub async fn register(
    State(state): State<Arc<AppState>>,
    Json(req): Json<RegisterRequest>,
) -> ApiResult<Json<serde_json::Value>> {
    if state.users.find_by_email(&req.email).await?.is_some() {
        return Err(ApiError::Conflict("Email already registered".into()));
    }
    if state.users.find_by_username(&req.username).await?.is_some() {
        return Err(ApiError::Conflict("Username already taken".into()));
    }

    let hash = hash_password(&req.password).map_err(ApiError::Auth)?;
    let user = state.users.create(
        &CreateUser { username: req.username, email: req.email, password: req.password, role: UserRole::Viewer },
        &hash,
    ).await?;

    Ok(Json(serde_json::json!({
        "message": "Registered successfully",
        "user": { "id": user.id, "username": user.username, "email": user.email }
    })))
}
