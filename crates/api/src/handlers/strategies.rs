use axum::{
    extract::{Extension, Path, State},
    Json,
};
use std::sync::Arc;
use uuid::Uuid;

use auth::middleware::AuthUser;
use db::models::strategy::{CreateStrategy, Strategy, StrategyStatus, UpdateStrategy};

use crate::{error::{ApiError, ApiResult}, state::AppState};

pub async fn list(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<AuthUser>,
) -> ApiResult<Json<Vec<Strategy>>> {
    let ss = state.strategies.list_by_user(user.id).await?;
    Ok(Json(ss))
}

pub async fn get(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Strategy>> {
    let s = state.strategies.find_by_id(id).await?
        .filter(|s| s.user_id == user.id)
        .ok_or_else(|| ApiError::NotFound(format!("Strategy {id}")))?;
    Ok(Json(s))
}

pub async fn create(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<AuthUser>,
    Json(req): Json<CreateStrategy>,
) -> ApiResult<Json<Strategy>> {
    let s = state.strategies.create(user.id, &req).await?;
    Ok(Json(s))
}

pub async fn update(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<AuthUser>,
    Path(id): Path<Uuid>,
    Json(req): Json<UpdateStrategy>,
) -> ApiResult<Json<Strategy>> {
    state.strategies.find_by_id(id).await?
        .filter(|s| s.user_id == user.id)
        .ok_or_else(|| ApiError::NotFound(format!("Strategy {id}")))?;

    let s = state.strategies.update(id, &req).await?
        .ok_or_else(|| ApiError::NotFound(format!("Strategy {id}")))?;
    Ok(Json(s))
}

pub async fn start(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<serde_json::Value>> {
    state.strategies.find_by_id(id).await?
        .filter(|s| s.user_id == user.id)
        .ok_or_else(|| ApiError::NotFound(format!("Strategy {id}")))?;

    let update = UpdateStrategy { status: Some(StrategyStatus::Active), ..Default::default() };
    state.strategies.update(id, &update).await?;

    Ok(Json(serde_json::json!({ "message": "Strategy started" })))
}

pub async fn pause(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<serde_json::Value>> {
    state.strategies.find_by_id(id).await?
        .filter(|s| s.user_id == user.id)
        .ok_or_else(|| ApiError::NotFound(format!("Strategy {id}")))?;

    let update = UpdateStrategy { status: Some(StrategyStatus::Paused), ..Default::default() };
    state.strategies.update(id, &update).await?;

    Ok(Json(serde_json::json!({ "message": "Strategy paused" })))
}

pub async fn delete(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<serde_json::Value>> {
    state.strategies.find_by_id(id).await?
        .filter(|s| s.user_id == user.id)
        .ok_or_else(|| ApiError::NotFound(format!("Strategy {id}")))?;

    state.strategies.delete(id).await?;
    Ok(Json(serde_json::json!({ "message": "Strategy deleted" })))
}
