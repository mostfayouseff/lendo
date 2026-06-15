use axum::{
    extract::{Extension, State},
    Json,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use auth::middleware::AuthUser;
use db::models::setting::{BotSettings, Setting};

use crate::{error::ApiResult, state::AppState};

pub async fn get_all(
    State(state): State<Arc<AppState>>,
) -> ApiResult<Json<Vec<Setting>>> {
    let ss = state.settings.get_all().await?;
    Ok(Json(ss))
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SetSettingRequest {
    pub key:   String,
    pub value: serde_json::Value,
}

pub async fn set(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<AuthUser>,
    Json(req): Json<SetSettingRequest>,
) -> ApiResult<Json<Setting>> {
    let s = state.settings.set(&req.key, req.value, Some(user.id)).await?;
    Ok(Json(s))
}

pub async fn get_bot_settings(
    State(state): State<Arc<AppState>>,
) -> ApiResult<Json<BotSettings>> {
    let enabled              = state.settings.get_bool("bot.enabled", false).await?;
    let mode                 = state.settings.get_string("bot.mode", "test").await?;
    let min_profit_lamports  = state.settings.get_i64("bot.min_profit_lamports", 10_000).await?;
    let max_position_lamports = state.settings.get_i64("bot.max_position_lamports", 1_000_000_000).await?;
    let slippage_bps         = state.settings.get_i64("bot.slippage_bps", 50).await? as i16;
    let max_hops             = state.settings.get_i64("bot.max_hops", 4).await? as i16;
    let flash_loan_enabled   = state.settings.get_bool("bot.flash_loan_enabled", false).await?;
    let flash_loan_provider  = state.settings.get_string("bot.flash_loan_provider", "solend").await?;
    let jito_tip_lamports    = state.settings.get_i64("bot.jito_tip_lamports", 1_000).await?;

    Ok(Json(BotSettings {
        enabled, mode, min_profit_lamports, max_position_lamports,
        slippage_bps, max_hops, flash_loan_enabled, flash_loan_provider, jito_tip_lamports,
    }))
}

pub async fn update_bot_settings(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<AuthUser>,
    Json(req): Json<BotSettings>,
) -> ApiResult<Json<BotSettings>> {
    let uid = Some(user.id);
    state.settings.set("bot.enabled",               serde_json::json!(req.enabled),              uid).await?;
    state.settings.set("bot.mode",                  serde_json::json!(req.mode),                 uid).await?;
    state.settings.set("bot.min_profit_lamports",   serde_json::json!(req.min_profit_lamports),  uid).await?;
    state.settings.set("bot.max_position_lamports", serde_json::json!(req.max_position_lamports),uid).await?;
    state.settings.set("bot.slippage_bps",          serde_json::json!(req.slippage_bps),         uid).await?;
    state.settings.set("bot.max_hops",              serde_json::json!(req.max_hops),             uid).await?;
    state.settings.set("bot.flash_loan_enabled",    serde_json::json!(req.flash_loan_enabled),   uid).await?;
    state.settings.set("bot.flash_loan_provider",   serde_json::json!(req.flash_loan_provider),  uid).await?;
    state.settings.set("bot.jito_tip_lamports",     serde_json::json!(req.jito_tip_lamports),    uid).await?;
    Ok(Json(req))
}
