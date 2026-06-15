use axum::{
    extract::{Path, Query, State},
    Json,
};
use serde::Deserialize;
use std::sync::Arc;
use uuid::Uuid;

use db::models::opportunity::{Opportunity, OpportunityStatus};

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
) -> ApiResult<Json<Vec<Opportunity>>> {
    let os = state.opps.list(page.limit, page.offset).await?;
    Ok(Json(os))
}

pub async fn list_recent(
    State(state): State<Arc<AppState>>,
) -> ApiResult<Json<Vec<Opportunity>>> {
    let os = state.opps.list_recent(24).await?;
    Ok(Json(os))
}
