use axum::{
    extract::{Path, Query, State},
    Json,
};
use serde::Deserialize;
use std::sync::Arc;
use uuid::Uuid;

use db::models::token::{CreateToken, Token, TokenStatus, UpdateToken};

use crate::{error::{ApiError, ApiResult}, state::AppState};

#[derive(Debug, Deserialize)]
pub struct Pagination {
    #[serde(default = "default_limit")]
    pub limit:  i64,
    #[serde(default)]
    pub offset: i64,
}

fn default_limit() -> i64 { 50 }

pub async fn list(
    State(state): State<Arc<AppState>>,
    Query(page): Query<Pagination>,
) -> ApiResult<Json<Vec<Token>>> {
    let tokens = state.tokens.list(page.limit, page.offset).await?;
    Ok(Json(tokens))
}

pub async fn list_active(
    State(state): State<Arc<AppState>>,
) -> ApiResult<Json<Vec<Token>>> {
    let tokens = state.tokens.list_active().await?;
    Ok(Json(tokens))
}

pub async fn get(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Token>> {
    let t = state.tokens.find_by_id(id).await?
        .ok_or_else(|| ApiError::NotFound(format!("Token {id}")))?;
    Ok(Json(t))
}

pub async fn create(
    State(state): State<Arc<AppState>>,
    Json(req): Json<CreateToken>,
) -> ApiResult<Json<Token>> {
    if state.tokens.find_by_mint(&req.mint_address).await?.is_some() {
        return Err(ApiError::Conflict(format!("Token {} already exists", req.mint_address)));
    }
    let t = state.tokens.create(&req).await?;
    Ok(Json(t))
}

pub async fn update(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
    Json(req): Json<UpdateToken>,
) -> ApiResult<Json<Token>> {
    let t = state.tokens.update(id, &req).await?
        .ok_or_else(|| ApiError::NotFound(format!("Token {id}")))?;
    Ok(Json(t))
}

pub async fn set_status(
    State(state): State<Arc<AppState>>,
    Path((id, status)): Path<(Uuid, String)>,
) -> ApiResult<Json<serde_json::Value>> {
    let s: TokenStatus = match status.as_str() {
        "active"      => TokenStatus::Active,
        "disabled"    => TokenStatus::Disabled,
        "blacklisted" => TokenStatus::Blacklisted,
        other         => return Err(ApiError::BadRequest(format!("Unknown status: {other}"))),
    };
    let ok = state.tokens.set_status(id, s).await?;
    if !ok { return Err(ApiError::NotFound(format!("Token {id}"))); }
    Ok(Json(serde_json::json!({ "message": "Token status updated" })))
}

pub async fn delete(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<serde_json::Value>> {
    let ok = state.tokens.delete(id).await?;
    if !ok { return Err(ApiError::NotFound(format!("Token {id}"))); }
    Ok(Json(serde_json::json!({ "message": "Token deleted" })))
}
