use axum::{
    extract::{Extension, Path, State},
    Json,
};
use std::sync::Arc;
use uuid::Uuid;

use auth::{middleware::AuthUser, password::hash_password};
use db::models::user::{PublicUser, UpdateUser, UserRole};

use crate::{error::{ApiError, ApiResult}, state::AppState};

pub async fn me(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<AuthUser>,
) -> ApiResult<Json<PublicUser>> {
    let u = state.users.find_by_id(user.id).await?
        .ok_or_else(|| ApiError::NotFound("User not found".into()))?;
    Ok(Json(u.into()))
}

pub async fn list(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<AuthUser>,
) -> ApiResult<Json<Vec<PublicUser>>> {
    if !matches!(user.role, UserRole::Admin) {
        return Err(ApiError::Forbidden("Admin role required".into()));
    }
    let users = state.users.list(100, 0).await?;
    Ok(Json(users.into_iter().map(Into::into).collect()))
}

pub async fn update(
    State(state): State<Arc<AppState>>,
    Extension(auth_user): Extension<AuthUser>,
    Path(id): Path<Uuid>,
    Json(req): Json<UpdateUser>,
) -> ApiResult<Json<PublicUser>> {
    if auth_user.id != id && !matches!(auth_user.role, UserRole::Admin) {
        return Err(ApiError::Forbidden("Cannot update other users".into()));
    }
    let u = state.users.update(id, &req).await?
        .ok_or_else(|| ApiError::NotFound(format!("User {id}")))?;
    Ok(Json(u.into()))
}

#[derive(serde::Deserialize)]
pub struct ChangePasswordRequest {
    pub current_password: String,
    pub new_password:     String,
}

pub async fn change_password(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<AuthUser>,
    Json(req): Json<ChangePasswordRequest>,
) -> ApiResult<Json<serde_json::Value>> {
    let u = state.users.find_by_id(user.id).await?
        .ok_or_else(|| ApiError::NotFound("User not found".into()))?;

    let ok = auth::password::verify_password(&req.current_password, &u.password_hash)
        .map_err(ApiError::Auth)?;
    if !ok {
        return Err(ApiError::Auth(auth::AuthError::InvalidCredentials));
    }

    if req.new_password.len() < 8 {
        return Err(ApiError::BadRequest("Password must be at least 8 characters".into()));
    }

    let hash = hash_password(&req.new_password).map_err(ApiError::Auth)?;
    state.users.update_password(user.id, &hash).await?;
    state.sessions.revoke_all_for_user(user.id).await?;

    Ok(Json(serde_json::json!({ "message": "Password changed; all sessions revoked" })))
}
