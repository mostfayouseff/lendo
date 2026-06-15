use axum::{extract::State, Json};
use prometheus::{
    Encoder, IntCounterVec, IntGaugeVec, Opts, Registry, TextEncoder,
};
use serde::Serialize;
use std::sync::Arc;

use crate::{error::ApiResult, state::AppState};

#[derive(Debug, Serialize)]
pub struct DashboardOverview {
    pub total_trades:           i64,
    pub confirmed_trades:       i64,
    pub failed_trades:          i64,
    pub total_profit_sol:       f64,
    pub win_rate:               f64,
    pub opportunities_today:    i64,
    pub active_strategies:      usize,
    pub unresolved_alerts:      usize,
    pub bot_running:            bool,
}

pub async fn overview(
    State(state): State<Arc<AppState>>,
) -> ApiResult<Json<DashboardOverview>> {
    let (summary, opps_today, strategies, alerts, bot_running) = tokio::try_join!(
        state.trades.summary(),
        state.opps.count_today(),
        state.strategies.list_active(),
        state.events.list_unresolved(),
        state.settings.get_bool("bot.enabled", false),
    )?;

    Ok(Json(DashboardOverview {
        total_trades:        summary.total_trades,
        confirmed_trades:    summary.confirmed_trades,
        failed_trades:       summary.failed_trades,
        total_profit_sol:    summary.total_profit_sol,
        win_rate:            summary.win_rate,
        opportunities_today: opps_today,
        active_strategies:   strategies.len(),
        unresolved_alerts:   alerts.len(),
        bot_running,
    }))
}

pub async fn metrics() -> String {
    let registry = Registry::new();

    let gauge_opts = Opts::new("apex_up", "Apex MEV platform running");
    let gauge = IntGaugeVec::new(gauge_opts, &["component"]).unwrap();
    let _ = registry.register(Box::new(gauge.clone()));
    gauge.with_label_values(&["api"]).set(1);

    let counter_opts = Opts::new("apex_requests_total", "Total HTTP requests");
    let counter = IntCounterVec::new(counter_opts, &["method", "path", "status"]).unwrap();
    let _ = registry.register(Box::new(counter.clone()));

    let encoder = TextEncoder::new();
    let mut buffer = Vec::new();
    let _ = encoder.encode(&registry.gather(), &mut buffer);
    String::from_utf8(buffer).unwrap_or_default()
}

pub async fn system_events(
    State(state): State<Arc<AppState>>,
) -> ApiResult<Json<Vec<db::models::SystemEvent>>> {
    let evs = state.events.list_unresolved().await?;
    Ok(Json(evs))
}

pub async fn resolve_event(
    State(state): State<Arc<AppState>>,
    axum::extract::Path(id): axum::extract::Path<uuid::Uuid>,
) -> ApiResult<Json<serde_json::Value>> {
    let ok = state.events.resolve(id).await?;
    if !ok { return Err(crate::error::ApiError::NotFound(format!("Event {id}"))); }
    Ok(Json(serde_json::json!({ "message": "Event resolved" })))
}
