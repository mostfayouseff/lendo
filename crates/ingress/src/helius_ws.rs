// =============================================================================
// HELIUS WEBSOCKET — PRIMARY LIVE INGRESS ENGINE
//
// Connects to: wss://mainnet.helius-rpc.com/?api-key=<KEY>
//
// Helius supports both standard Solana WebSocket methods AND enhanced methods:
//   - logsSubscribe (standard Solana — filtered by program mentions)
//   - transactionSubscribe (Helius-specific — full transaction data)
//
// Strategy: Subscribe to transactionSubscribe for all major DEX programs.
// Each transaction notification triggers a ShredEvent for the hot loop.
// Price discovery happens separately via the multi-source price monitor.
//
// SELF-HEALING: Exponential back-off reconnect with no max retry limit.
// FALLBACK: If Helius fails, use Alchemy WebSocket (standard logsSubscribe).
//
// LOGGING: All events logged with "LIVE DATA SOURCE: HELIUS" prefix.
// =============================================================================

use crate::shredstream::ShredEvent;
use bytes::Bytes;
use serde::Deserialize;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::time::sleep;
use tokio_tungstenite::connect_async_tls_with_config;
use tracing::{debug, error, info, warn};

const RECONNECT_BASE_DELAY_MS: u64 = 500;
const RECONNECT_MAX_DELAY_MS: u64 = 30_000;

const HELIUS_WS_HOST: &str = "mainnet.helius-rpc.com";
const ALCHEMY_WS_HOST: &str = "solana-mainnet.g.alchemy.com";

/// Solana mainnet DEX program IDs monitored via logsSubscribe / transactionSubscribe.
pub const DEX_PROGRAMS: &[&str] = &[
    "675kPX9MHTjS2zt1qfr1NYHuzeLXfQM9H24wFSUt1Mp8", // Raydium AMM v4
    "CAMMCzo5YL8w4VFF8KVHrK22GGUsp5VTaW7grrKgrWqK", // Raydium CLMM
    "whirLbMiicVdio4qvUfM5KAg6Ct8VwpYzGff3sFjJ37",  // Orca Whirlpools
    "LBUZKhRxPF3XUpBCjp4YzTKgLccjZhTSDM9YuVaPwxo",  // Meteora DLMM
    "Eo7WjKq67rjJQSZxS6z3YkapzY3eMj6Xy8X5EQVn5UaB", // Meteora Dynamic AMM
    "PhoeNiXZ8ByJGLkxNfZRnkUfjvmuYqLR89jjFHGqdXY",  // Phoenix DEX
    "JUP6LkbZbjS1jKKwapdHNy74zcZ3tLUZoi5QNyVTaV4",  // Jupiter V6
];

// ── JSON structures ─────────────────────────────────────────────────────────────

#[derive(Deserialize, Debug)]
struct WsMessage {
    method: Option<String>,
    params: Option<serde_json::Value>,
    result: Option<serde_json::Value>,
    id: Option<u64>,
    error: Option<WsError>,
}

#[derive(Deserialize, Debug)]
struct WsError {
    code: i64,
    message: String,
}

// ── Helius WebSocket Stream (PRIMARY) ───────────────────────────────────────────

/// Primary real-time stream using Helius Enhanced WebSocket.
///
/// Connects to `wss://mainnet.helius-rpc.com/?api-key=<HELIUS_API_KEY>`.
/// Uses both `logsSubscribe` (standard) and `transactionSubscribe` (Helius-enhanced)
/// to monitor all major DEX programs for arbitrage opportunities.
///
/// Emits `ShredEvent` for every DEX transaction detected on mainnet.
pub struct HeliusTransactionStream;

impl HeliusTransactionStream {
    /// Spawn the Helius stream. Returns the receiver channel.
    #[must_use]
    pub fn spawn(api_key: String) -> mpsc::Receiver<ShredEvent> {
        let (tx, rx) = mpsc::channel(8192);
        tokio::spawn(Self::run(tx, api_key));
        rx
    }

    async fn run(tx: mpsc::Sender<ShredEvent>, api_key: String) {
        let url = format!("wss://{}/?api-key={}", HELIUS_WS_HOST, api_key);
        let mut reconnect_delay_ms = RECONNECT_BASE_DELAY_MS;
        let mut connect_count: u64 = 0;

        info!(
            host     = HELIUS_WS_HOST,
            programs = DEX_PROGRAMS.len(),
            "LIVE DATA SOURCE: HELIUS — starting primary WebSocket stream"
        );

        loop {
            connect_count += 1;
            info!(
                attempt = connect_count,
                host    = HELIUS_WS_HOST,
                "Helius WS: connecting to live mainnet stream"
            );

            match Self::connect_and_stream(&url, &tx).await {
                Ok(()) => {
                    info!("Helius WS: stream ended cleanly — reconnecting");
                    reconnect_delay_ms = RECONNECT_BASE_DELAY_MS;
                }
                Err(e) => {
                    warn!(
                        error    = %e,
                        delay_ms = reconnect_delay_ms,
                        "Helius WS: connection error — retrying (self-healing)"
                    );
                }
            }

            if tx.is_closed() {
                info!("Helius WS: receiver dropped — stopping stream");
                return;
            }

            sleep(Duration::from_millis(reconnect_delay_ms)).await;
            reconnect_delay_ms = (reconnect_delay_ms * 2).min(RECONNECT_MAX_DELAY_MS);
        }
    }

    async fn connect_and_stream(
        url: &str,
        tx: &mpsc::Sender<ShredEvent>,
    ) -> anyhow::Result<()> {
        let (mut ws, _) = connect_async_tls_with_config(url, None, false, None).await?;

        info!(
            host     = HELIUS_WS_HOST,
            programs = DEX_PROGRAMS.len(),
            "Helius WS: connected — subscribing to DEX program logs"
        );

        // Subscribe to logsSubscribe for each DEX program
        // Helius supports standard Solana WebSocket methods
        for (i, &program_id) in DEX_PROGRAMS.iter().enumerate() {
            Self::send_logs_subscribe(&mut ws, i as u64 + 1, program_id).await?;
        }

        // Also try transactionSubscribe (Helius-enhanced method) for richer data
        // This gives full transaction data including account changes
        let tx_sub_id = DEX_PROGRAMS.len() as u64 + 100;
        Self::send_transaction_subscribe(&mut ws, tx_sub_id).await?;

        info!(
            programs   = DEX_PROGRAMS.len(),
            host       = HELIUS_WS_HOST,
            "Helius WS: LIVE — logsSubscribe + transactionSubscribe active for all DEX programs"
        );

        let mut event_count: u64 = 0;

        use futures_util::StreamExt;
        while let Some(msg_result) = ws.next().await {
            let msg = match msg_result {
                Ok(m) => m,
                Err(e) => {
                    warn!(error = %e, "Helius WS: message error");
                    return Err(e.into());
                }
            };

            use tokio_tungstenite::tungstenite::Message;
            let text = match msg {
                Message::Text(t) => t,
                Message::Ping(data) => {
                    use futures_util::SinkExt;
                    let _ = ws.send(Message::Pong(data)).await;
                    continue;
                }
                Message::Close(_) => {
                    info!("Helius WS: server closed connection — reconnecting");
                    return Ok(());
                }
                _ => continue,
            };

            let notification: WsMessage = match serde_json::from_str(&text) {
                Ok(n) => n,
                Err(e) => {
                    debug!(error = %e, "Helius WS: JSON parse error (skipping)");
                    continue;
                }
            };

            // ── Subscription errors ────────────────────────────────────────────
            if let Some(err) = &notification.error {
                error!(
                    code    = err.code,
                    message = %err.message,
                    "Helius WS: subscription error"
                );
                // Code -32601 = method not found (transactionSubscribe not supported in this tier)
                // Continue — logsSubscribe subscriptions may still be active
                continue;
            }

            // ── Subscription confirmations ─────────────────────────────────────
            if notification.method.is_none() {
                if let Some(result) = &notification.result {
                    if let Some(sub_id) = result.as_u64() {
                        info!(
                            subscription_id = sub_id,
                            request_id      = notification.id,
                            "Helius WS: subscription confirmed"
                        );
                    }
                }
                continue;
            }

            // ── Log notification (logsNotification) ───────────────────────────
            if notification.method.as_deref() == Some("logsNotification") {
                let slot = extract_slot_from_params(&notification.params);
                debug!(slot, "LIVE DATA SOURCE: HELIUS — DEX log event");

                event_count += 1;
                let event = ShredEvent {
                    slot,
                    index: (event_count & 0xFFFF_FFFF) as u32,
                    data: Bytes::from_static(b"helius_log_trigger"),
                };

                if tx.send(event).await.is_err() {
                    info!("Helius WS: main loop dropped receiver — stopping");
                    return Ok(());
                }
            }

            // ── Transaction notification (transactionNotification) ─────────────
            if notification.method.as_deref() == Some("transactionNotification") {
                let slot = extract_slot_from_params(&notification.params);
                debug!(slot, "LIVE DATA SOURCE: HELIUS — DEX transaction event");

                event_count += 1;
                let event = ShredEvent {
                    slot,
                    index: (event_count & 0xFFFF_FFFF) as u32,
                    data: Bytes::from_static(b"helius_tx_trigger"),
                };

                if tx.send(event).await.is_err() {
                    info!("Helius WS: main loop dropped receiver — stopping");
                    return Ok(());
                }
            }
        }

        Ok(())
    }

    async fn send_logs_subscribe(
        ws: &mut (impl futures_util::Sink<tokio_tungstenite::tungstenite::Message, Error = tokio_tungstenite::tungstenite::Error> + Unpin),
        id: u64,
        program_id: &str,
    ) -> anyhow::Result<()> {
        use futures_util::SinkExt;
        use tokio_tungstenite::tungstenite::Message;

        let msg = serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "logsSubscribe",
            "params": [
                { "mentions": [program_id] },
                { "commitment": "processed" }
            ]
        });

        debug!(program_id = %program_id, id, "Helius WS: sending logsSubscribe");
        ws.send(Message::Text(msg.to_string())).await?;
        Ok(())
    }

    async fn send_transaction_subscribe(
        ws: &mut (impl futures_util::Sink<tokio_tungstenite::tungstenite::Message, Error = tokio_tungstenite::tungstenite::Error> + Unpin),
        id: u64,
    ) -> anyhow::Result<()> {
        use futures_util::SinkExt;
        use tokio_tungstenite::tungstenite::Message;

        // Helius transactionSubscribe — streams full transactions matching filter
        let msg = serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "transactionSubscribe",
            "params": [
                {
                    "accountInclude": DEX_PROGRAMS
                },
                {
                    "commitment": "processed",
                    "encoding": "base64",
                    "transactionDetails": "signatures",
                    "showRewards": false,
                    "maxSupportedTransactionVersion": 0
                }
            ]
        });

        debug!(id, "Helius WS: sending transactionSubscribe for all DEX programs");
        ws.send(Message::Text(msg.to_string())).await?;
        Ok(())
    }
}

// ── Alchemy WebSocket Stream (FALLBACK) ─────────────────────────────────────────

/// Fallback real-time stream using Alchemy standard Solana WebSocket.
///
/// Connects to `wss://solana-mainnet.g.alchemy.com/v2/<ALCHEMY_API_KEY>`.
/// Uses standard `logsSubscribe` for all major DEX programs.
pub struct AlchemyTransactionStream;

impl AlchemyTransactionStream {
    /// Spawn the Alchemy stream. Returns the receiver channel.
    #[must_use]
    pub fn spawn(api_key: String) -> mpsc::Receiver<ShredEvent> {
        let (tx, rx) = mpsc::channel(8192);
        tokio::spawn(Self::run(tx, api_key));
        rx
    }

    async fn run(tx: mpsc::Sender<ShredEvent>, api_key: String) {
        let url = format!("wss://{}/v2/{}", ALCHEMY_WS_HOST, api_key);
        let mut reconnect_delay_ms = RECONNECT_BASE_DELAY_MS;
        let mut connect_count: u64 = 0;

        info!(
            host     = ALCHEMY_WS_HOST,
            programs = DEX_PROGRAMS.len(),
            "LIVE DATA SOURCE: ALCHEMY (FALLBACK) — starting fallback WebSocket stream"
        );

        loop {
            connect_count += 1;
            info!(
                attempt = connect_count,
                host    = ALCHEMY_WS_HOST,
                "Alchemy WS: connecting (fallback stream)"
            );

            match Self::connect_and_stream(&url, &tx).await {
                Ok(()) => {
                    info!("Alchemy WS: stream ended cleanly — reconnecting");
                    reconnect_delay_ms = RECONNECT_BASE_DELAY_MS;
                }
                Err(e) => {
                    warn!(
                        error    = %e,
                        delay_ms = reconnect_delay_ms,
                        "Alchemy WS: connection error — retrying (self-healing)"
                    );
                }
            }

            if tx.is_closed() {
                info!("Alchemy WS: receiver dropped — stopping stream");
                return;
            }

            sleep(Duration::from_millis(reconnect_delay_ms)).await;
            reconnect_delay_ms = (reconnect_delay_ms * 2).min(RECONNECT_MAX_DELAY_MS);
        }
    }

    async fn connect_and_stream(
        url: &str,
        tx: &mpsc::Sender<ShredEvent>,
    ) -> anyhow::Result<()> {
        let (mut ws, _) = connect_async_tls_with_config(url, None, false, None).await?;

        for (i, &program_id) in DEX_PROGRAMS.iter().enumerate() {
            use futures_util::SinkExt;
            use tokio_tungstenite::tungstenite::Message;

            let msg = serde_json::json!({
                "jsonrpc": "2.0",
                "id": i as u64 + 1,
                "method": "logsSubscribe",
                "params": [
                    { "mentions": [program_id] },
                    { "commitment": "processed" }
                ]
            });
            ws.send(Message::Text(msg.to_string())).await?;
        }

        info!(
            programs = DEX_PROGRAMS.len(),
            host     = ALCHEMY_WS_HOST,
            "Alchemy WS: logsSubscribe active for all DEX programs (fallback)"
        );

        let mut event_count: u64 = 0;

        use futures_util::StreamExt;
        while let Some(msg_result) = ws.next().await {
            let msg = match msg_result {
                Ok(m) => m,
                Err(e) => {
                    warn!(error = %e, "Alchemy WS: message error");
                    return Err(e.into());
                }
            };

            use tokio_tungstenite::tungstenite::Message;
            let text = match msg {
                Message::Text(t) => t,
                Message::Ping(data) => {
                    use futures_util::SinkExt;
                    let _ = ws.send(Message::Pong(data)).await;
                    continue;
                }
                Message::Close(_) => {
                    return Ok(());
                }
                _ => continue,
            };

            let notification: WsMessage = match serde_json::from_str(&text) {
                Ok(n) => n,
                Err(_) => continue,
            };

            if let Some(err) = &notification.error {
                error!(code = err.code, message = %err.message, "Alchemy WS: subscription error");
                continue;
            }

            if notification.method.as_deref() == Some("logsNotification") {
                let slot = extract_slot_from_params(&notification.params);
                event_count += 1;
                let event = ShredEvent {
                    slot,
                    index: (event_count & 0xFFFF_FFFF) as u32,
                    data: Bytes::from_static(b"alchemy_log_trigger"),
                };

                if tx.send(event).await.is_err() {
                    return Ok(());
                }
            }
        }

        Ok(())
    }
}

// ── Utility ──────────────────────────────────────────────────────────────────

fn extract_slot_from_params(params: &Option<serde_json::Value>) -> u64 {
    params
        .as_ref()
        .and_then(|p| p.get("result"))
        .and_then(|r| r.get("context"))
        .and_then(|c| c.get("slot"))
        .and_then(|s| s.as_u64())
        .unwrap_or(0)
}
