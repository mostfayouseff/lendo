// =============================================================================
// SECURITY AUDIT CHECKLIST — ingress/src/helius_ws.rs (Alchemy WebSocket)
// [✓] API key transmitted only as WSS path param (TLS-encrypted)
// [✓] All parse errors handled — never panic on malformed JSON
// [✓] Reconnect loop: exponential back-off, max 60s between attempts
// [✓] Channel backpressure: bounded mpsc(8192) prevents OOM
// [✓] No unsafe code
// [✓] Helius removed — uses Alchemy standard Solana WS only
//
// ALCHEMY WEBSOCKET — standard Solana logsSubscribe
//
// Endpoint: wss://solana-mainnet.g.alchemy.com/v2/<ALCHEMY_API_KEY>
//
// Unlike Helius, Alchemy uses standard Solana WebSocket methods:
//   - logsSubscribe (filtered by mentions = [program_id])
//   - No transactionSubscribe (Helius-only, not supported here)
//
// We open multiple logsSubscribe subscriptions on the same connection:
//   - Jupiter V6       (highest volume)
//   - Raydium AMM v4   (second highest)
//   - Orca Whirlpools  (third)
//   - Meteora DLMM
//   - Phoenix DEX
//
// Each log notification from any DEX program emits a ShredEvent to the hot loop.
// Price discovery happens separately via the Jupiter Price API monitor.
//
// Fallback: if ALCHEMY_API_KEY not set → MockShredStream (as before).
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
const RECONNECT_MAX_DELAY_MS:  u64 = 60_000;
const ALCHEMY_WS_HOST:         &str = "solana-mainnet.g.alchemy.com";

/// Solana mainnet DEX program IDs monitored via logsSubscribe.
pub const DEX_PROGRAMS: &[&str] = &[
    "675kPX9MHTjS2zt1qfr1NYHuzeLXfQM9H24wFSUt1Mp8", // Raydium AMM v4
    "CAMMCzo5YL8w4VFF8KVHrK22GGUsp5VTaW7grrKgrWqK", // Raydium CLMM
    "whirLbMiicVdio4qvUfM5KAg6Ct8VwpYzGff3sFjJ37",  // Orca Whirlpools
    "LBUZKhRxPF3XUpBCjp4YzTKgLccjZhTSDM9YuVaPwxo",  // Meteora DLMM
    "Eo7WjKq67rjJQSZxS6z3YkapzY3eMj6Xy8X5EQVn5UaB", // Meteora Dynamic AMM
    "PhoeNiXZ8ByJGLkxNfZRnkUfjvmuYqLR89jjFHGqdXY",  // Phoenix DEX
    "JUP6LkbZbjS1jKKwapdHNy74zcZ3tLUZoi5QNyVTaV4",  // Jupiter V6
];

// ── JSON structures ────────────────────────────────────────────────────────────

#[derive(Deserialize, Debug)]
struct WsMessage {
    method: Option<String>,
    params: Option<serde_json::Value>,
    result: Option<serde_json::Value>,
    id:     Option<u64>,
    error:  Option<WsError>,
}

#[derive(Deserialize, Debug)]
struct WsError {
    code:    i64,
    message: String,
}

// ── Alchemy WebSocket stream ────────────────────────────────────────────────────

/// Real-time Alchemy WebSocket stream using standard Solana logsSubscribe.
///
/// Connects to `wss://solana-mainnet.g.alchemy.com/v2/<ALCHEMY_API_KEY>` and
/// opens logsSubscribe subscriptions for every major Solana DEX program.
///
/// Each DEX log notification emits a `ShredEvent` to trigger the hot loop.
/// If ALCHEMY_API_KEY is not set, callers should use `MockShredStream` instead.
pub struct AlchemyTransactionStream;

/// Backward-compatible alias — external code can still refer to HeliusTransactionStream.
pub type HeliusTransactionStream = AlchemyTransactionStream;

impl AlchemyTransactionStream {
    /// Spawn the stream. Returns the receiver channel.
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

        loop {
            connect_count += 1;
            info!(
                attempt = connect_count,
                host    = ALCHEMY_WS_HOST,
                "Alchemy WS: connecting"
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
                        "Alchemy WS: connection error — will retry"
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
        tx:  &mpsc::Sender<ShredEvent>,
    ) -> anyhow::Result<()> {
        let (mut ws, _) = connect_async_tls_with_config(url, None, false, None).await?;

        // Subscribe to logs for each DEX program (standard Solana logsSubscribe).
        // Alchemy supports multiple subscriptions on a single connection.
        for (i, &program_id) in DEX_PROGRAMS.iter().enumerate() {
            Self::send_logs_subscribe(&mut ws, i as u64 + 1, program_id).await?;
        }

        info!(
            programs   = DEX_PROGRAMS.len(),
            host       = ALCHEMY_WS_HOST,
            "Alchemy WS: logsSubscribe sent for all DEX programs"
        );

        let mut event_count: u64 = 0;

        use futures_util::StreamExt;
        while let Some(msg_result) = ws.next().await {
            let msg = match msg_result {
                Ok(m)  => m,
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
                    info!("Alchemy WS: server closed connection");
                    return Ok(());
                }
                _ => continue,
            };

            let notification: WsMessage = match serde_json::from_str(&text) {
                Ok(n)  => n,
                Err(e) => {
                    debug!(error = %e, "Alchemy WS: JSON parse error (skipping)");
                    continue;
                }
            };

            // ── Subscription errors ──────────────────────────────────────────
            if let Some(err) = &notification.error {
                error!(
                    code    = err.code,
                    message = %err.message,
                    "Alchemy WS: subscription error"
                );
                // Continue — other subscriptions may still be valid
                continue;
            }

            // ── Subscription confirmations ───────────────────────────────────
            if notification.method.is_none() {
                if let Some(result) = &notification.result {
                    if let Some(sub_id) = result.as_u64() {
                        info!(
                            subscription_id = sub_id,
                            request_id      = notification.id,
                            "Alchemy WS: subscription confirmed"
                        );
                    }
                }
                continue;
            }

            // ── Log notification (logsNotification) ──────────────────────────
            if notification.method.as_deref() == Some("logsNotification") {
                let slot = notification
                    .params
                    .as_ref()
                    .and_then(|p| p.get("result"))
                    .and_then(|r| r.get("context"))
                    .and_then(|c| c.get("slot"))
                    .and_then(|s| s.as_u64())
                    .unwrap_or(0);

                debug!(slot, "Alchemy WS: DEX log notification");

                event_count += 1;
                let event = ShredEvent {
                    slot,
                    index: (event_count & 0xFFFF_FFFF) as u32,
                    data:  Bytes::from_static(b"alchemy_log_trigger"),
                };

                if tx.send(event).await.is_err() {
                    info!("Alchemy WS: main loop dropped receiver — stopping");
                    return Ok(());
                }
            }
        }

        Ok(())
    }

    /// Send a logsSubscribe request for a single program ID.
    async fn send_logs_subscribe(
        ws:         &mut (impl futures_util::Sink<tokio_tungstenite::tungstenite::Message, Error = tokio_tungstenite::tungstenite::Error> + Unpin),
        id:         u64,
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

        debug!(program_id = %program_id, id, "Alchemy WS: sending logsSubscribe");
        ws.send(Message::Text(msg.to_string())).await?;
        Ok(())
    }
}
