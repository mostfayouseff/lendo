use axum::{
    extract::{Path, Query, State},
    Json,
};
use serde::Deserialize;
use std::sync::Arc;
use uuid::Uuid;

use db::models::trade::{Trade, TradeSummary};

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
) -> ApiResult<Json<Vec<Trade>>> {
    let ts = state.trades.list(page.limit, page.offset).await?;
    Ok(Json(ts))
}

pub async fn get(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Trade>> {
    let t = state.trades.find_by_id(id).await?
        .ok_or_else(|| ApiError::NotFound(format!("Trade {id}")))?;
    Ok(Json(t))
}

pub async fn summary(
    State(state): State<Arc<AppState>>,
) -> ApiResult<Json<TradeSummary>> {
    let s = state.trades.summary().await?;
    Ok(Json(s))
}
