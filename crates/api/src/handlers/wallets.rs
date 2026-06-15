use axum::{
    extract::{Extension, Path, State},
    Json,
};
use std::sync::Arc;
use uuid::Uuid;

use auth::middleware::AuthUser;
use db::models::wallet::{CreateWallet, PublicWallet, UpdateWallet, WalletType};

use crate::{error::{ApiError, ApiResult}, state::AppState};

pub async fn list(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<AuthUser>,
) -> ApiResult<Json<Vec<PublicWallet>>> {
    let wallets = state.wallets.list_by_user(user.id).await?;
    Ok(Json(wallets.into_iter().map(Into::into).collect()))
}

pub async fn get(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<PublicWallet>> {
    let w = state.wallets.find_by_id(id).await?
        .filter(|w| w.user_id == user.id)
        .ok_or_else(|| ApiError::NotFound(format!("Wallet {id}")))?;
    Ok(Json(w.into()))
}

#[derive(serde::Deserialize)]
pub struct CreateWalletRequest {
    pub label:       String,
    pub wallet_type: WalletType,
    pub secret:      String,
}

pub async fn create(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<AuthUser>,
    Json(req): Json<CreateWalletRequest>,
) -> ApiResult<Json<PublicWallet>> {
    let address = derive_address_from_secret(&req.secret)
        .map_err(|e| ApiError::BadRequest(format!("Invalid secret: {e}")))?;

    let encrypted = encrypt_secret(&req.secret);

    let w = state.wallets.create(
        user.id,
        &CreateWallet { label: req.label, wallet_type: req.wallet_type, secret: req.secret },
        &address,
        &encrypted,
    ).await?;

    Ok(Json(w.into()))
}

pub async fn activate(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<serde_json::Value>> {
    state.wallets.find_by_id(id).await?
        .filter(|w| w.user_id == user.id)
        .ok_or_else(|| ApiError::NotFound(format!("Wallet {id}")))?;

    state.wallets.activate(user.id, id).await?;
    Ok(Json(serde_json::json!({ "message": "Wallet activated" })))
}

pub async fn update(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<AuthUser>,
    Path(id): Path<Uuid>,
    Json(req): Json<UpdateWallet>,
) -> ApiResult<Json<PublicWallet>> {
    state.wallets.find_by_id(id).await?
        .filter(|w| w.user_id == user.id)
        .ok_or_else(|| ApiError::NotFound(format!("Wallet {id}")))?;

    let w = state.wallets.update(id, &req).await?
        .ok_or_else(|| ApiError::NotFound(format!("Wallet {id}")))?;
    Ok(Json(w.into()))
}

pub async fn delete(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<serde_json::Value>> {
    let w = state.wallets.find_by_id(id).await?
        .filter(|w| w.user_id == user.id)
        .ok_or_else(|| ApiError::NotFound(format!("Wallet {id}")))?;

    if w.is_active {
        return Err(ApiError::BadRequest("Cannot delete the active wallet".into()));
    }

    state.wallets.delete(id).await?;
    Ok(Json(serde_json::json!({ "message": "Wallet deleted" })))
}

fn derive_address_from_secret(secret: &str) -> anyhow::Result<String> {
    use base64::engine::general_purpose::STANDARD;
    use base64::Engine;

    let trimmed = secret.trim();
    if let Ok(bytes) = STANDARD.decode(trimmed) {
        if bytes.len() == 64 {
            let pubkey_bytes = &bytes[32..64];
            return Ok(bs58::encode(pubkey_bytes).into_string());
        }
    }
    if let Ok(bytes) = serde_json::from_str::<Vec<u8>>(trimmed) {
        if bytes.len() == 64 {
            let pubkey_bytes = &bytes[32..64];
            return Ok(bs58::encode(pubkey_bytes).into_string());
        }
    }
    Err(anyhow::anyhow!("Could not derive public key from provided secret"))
}

fn encrypt_secret(secret: &str) -> String {
    use base64::engine::general_purpose::STANDARD;
    use base64::Engine;
    STANDARD.encode(format!("enc:{secret}"))
}
