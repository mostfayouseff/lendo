use axum::{
    extract::{Extension, Path, State},
    Json,
};
use std::sync::Arc;
use uuid::Uuid;

use auth::middleware::AuthUser;
use db::models::risk_rule::{CreateRiskRule, RiskRule, UpdateRiskRule};

use crate::{error::{ApiError, ApiResult}, state::AppState};

pub async fn list(
    State(state): State<Arc<AppState>>,
) -> ApiResult<Json<Vec<RiskRule>>> {
    let rs = state.risk_rules.list().await?;
    Ok(Json(rs))
}

pub async fn create(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<AuthUser>,
    Json(req): Json<CreateRiskRule>,
) -> ApiResult<Json<RiskRule>> {
    let r = state.risk_rules.create(user.id, &req).await?;
    Ok(Json(r))
}

pub async fn update(
    State(state): State<Arc<AppState>>,
    Extension(_user): Extension<AuthUser>,
    Path(id): Path<Uuid>,
    Json(req): Json<UpdateRiskRule>,
) -> ApiResult<Json<RiskRule>> {
    let r = state.risk_rules.update(id, &req).await?
        .ok_or_else(|| ApiError::NotFound(format!("Risk rule {id}")))?;
    Ok(Json(r))
}

pub async fn delete(
    State(state): State<Arc<AppState>>,
    Extension(_user): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<serde_json::Value>> {
    let ok = state.risk_rules.delete(id).await?;
    if !ok { return Err(ApiError::NotFound(format!("Risk rule {id}"))); }
    Ok(Json(serde_json::json!({ "message": "Risk rule deleted" })))
}
