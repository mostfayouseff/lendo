use axum::{
    extract::{Extension, State},
    Json,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use auth::middleware::AuthUser;
use crate::{
    error::{ApiError, ApiResult},
    state::AppState,
    ws::WsEvent,
};

#[derive(Debug, Serialize)]
pub struct BotStatusResponse {
    pub running:    bool,
    pub mode:       String,
    pub uptime_sec: u64,
}

pub async fn status(
    State(state): State<Arc<AppState>>,
) -> ApiResult<Json<BotStatusResponse>> {
    let running = state.settings.get_bool("bot.enabled", false).await?;
    let mode    = state.settings.get_string("bot.mode", "test").await?;
    Ok(Json(BotStatusResponse { running, mode, uptime_sec: 0 }))
}

#[derive(Debug, Deserialize)]
pub struct BotCommandRequest {
    pub command: String,
}

pub async fn command(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<AuthUser>,
    Json(req): Json<BotCommandRequest>,
) -> ApiResult<Json<serde_json::Value>> {
    use db::models::user::UserRole;
    if !matches!(user.role, UserRole::Admin | UserRole::Trader) {
        return Err(ApiError::Forbidden("Admin or Trader role required".into()));
    }

    match req.command.as_str() {
        "start" => {
            state.settings.set("bot.enabled", serde_json::json!(true), Some(user.id)).await?;
            state.broadcast(WsEvent::BotStatus {
                running: true,
                mode: state.settings.get_string("bot.mode", "test").await?,
            });
        }
        "stop" => {
            state.settings.set("bot.enabled", serde_json::json!(false), Some(user.id)).await?;
            state.broadcast(WsEvent::BotStatus {
                running: false,
                mode: state.settings.get_string("bot.mode", "test").await?,
            });
        }
        "emergency_stop" => {
            state.settings.set("bot.enabled", serde_json::json!(false), Some(user.id)).await?;
            state.broadcast(WsEvent::CircuitBreaker {
                triggered: true,
                reason: "Emergency stop issued by operator".to_string(),
            });
        }
        other => return Err(ApiError::BadRequest(format!("Unknown command: {other}"))),
    }

    Ok(Json(serde_json::json!({ "message": format!("Command '{}' executed", req.command) })))
}
