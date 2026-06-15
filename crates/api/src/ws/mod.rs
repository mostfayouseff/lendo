use axum::{
    extract::{ws::{Message, WebSocket, WebSocketUpgrade}, State},
    response::IntoResponse,
};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::broadcast;
use tracing::{debug, info, warn};

use crate::state::AppState;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WsEvent {
    TradeExecuted    { trade_id: String, profit_lamports: i64, status: String },
    OpportunityFound { path: Vec<String>, profit_lamports: i64 },
    BotStatus        { running: bool, mode: String },
    SystemAlert      { severity: String, message: String },
    PriceUpdate      { mint: String, price_usd: f64 },
    WalletBalance    { address: String, balance_lamports: i64 },
    CircuitBreaker   { triggered: bool, reason: String },
}

pub async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_socket(socket, state.ws_tx.subscribe()))
}

async fn handle_socket(socket: WebSocket, mut rx: broadcast::Receiver<WsEvent>) {
    info!("WebSocket client connected");
    let (mut sender, mut receiver) = socket.split();

    let mut send_task = tokio::spawn(async move {
        while let Ok(event) = rx.recv().await {
            match serde_json::to_string(&event) {
                Ok(json) => {
                    if sender.send(Message::Text(json.into())).await.is_err() {
                        break;
                    }
                }
                Err(e) => warn!("WS serialize error: {e}"),
            }
        }
    });

    let mut recv_task = tokio::spawn(async move {
        while let Some(Ok(msg)) = receiver.next().await {
            match msg {
                Message::Ping(p) => debug!("WS ping: {p:?}"),
                Message::Close(_) => break,
                _ => {}
            }
        }
    });

    tokio::select! {
        _ = &mut send_task => recv_task.abort(),
        _ = &mut recv_task => send_task.abort(),
    }

    info!("WebSocket client disconnected");
}
