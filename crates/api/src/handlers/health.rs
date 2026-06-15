use axum::{extract::State, Json};
use serde_json::{json, Value};
use std::sync::Arc;

use crate::state::AppState;

pub async fn health(State(state): State<Arc<AppState>>) -> Json<Value> {
    let db_ok    = state.db.health_check().await.is_ok();
    let cache_ok = state.cache.health_check().await.is_ok();

    Json(json!({
        "status":  if db_ok && cache_ok { "ok" } else { "degraded" },
        "version": env!("CARGO_PKG_VERSION"),
        "checks": {
            "database": if db_ok    { "ok" } else { "error" },
            "cache":    if cache_ok { "ok" } else { "error" },
        }
    }))
}

pub async fn ready() -> Json<Value> {
    Json(json!({ "ready": true }))
}
