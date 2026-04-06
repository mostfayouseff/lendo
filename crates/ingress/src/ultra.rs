// crates/ingress/src/ultra.rs
// Jupiter Ultra / Swap V2 — Production Safe + Rate Limit Robust (April 2026)

use anyhow::anyhow;
use base64::Engine as _;
use once_cell::sync::Lazy;
use rand;
use reqwest::Client;
use serde::Deserialize;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;
use tokio::time::sleep;
use tracing::{debug, info, warn};

const ULTRA_TIMEOUT_MS: u64 = 15_000;
const ULTRA_BASE_URL: &str = "https://api.jup.ag/swap/v2/order";
const MAX_RETRIES: u8 = 5;

// Global throttle for Ultra (prevents burst calls)
static GLOBAL_ULTRA_THROTTLE: Lazy<Arc<Mutex<Instant>>> =
    Lazy::new(|| Arc::new(Mutex::new(Instant::now())));

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UltraOrderResponse {
    pub input_mint: Option<String>,
    pub output_mint: Option<String>,
    pub in_amount: Option<String>,
    pub out_amount: Option<String>,
    pub other_amount_threshold: Option<String>,
    pub slippage_bps: Option<u64>,
    pub price_impact_pct: Option<String>,
    pub transaction: Option<String>,   // base64
    pub request_id: Option<String>,
}

#[derive(Debug, Clone)]
pub struct RouteExecutionData {
    pub input_mint: String,
    pub output_mint: String,
    pub in_amount: u64,
    pub out_amount: u64,
    pub other_amount_threshold: u64,
    pub price_impact_pct: f64,
    pub slippage_bps: u64,
    pub transaction_b64: String,
    pub transaction_bytes: Vec<u8>,
    pub request_id: String,
    pub total_fee_lamports: u64,
}

pub fn build_ultra_client() -> anyhow::Result<Client> {
    Ok(Client::builder()
        .timeout(Duration::from_millis(ULTRA_TIMEOUT_MS))
        .pool_max_idle_per_host(8)
        .build()?)
}

pub async fn get_best_route_and_transaction(
    client: &Client,
    input_mint: &str,
    output_mint: &str,
    amount: u64,
    taker: &str,
    api_key: Option<&str>,
    slippage_bps: u16,
) -> anyhow::Result<RouteExecutionData> {

    // Global throttle to avoid 429 bursts
    {
        let mut last = GLOBAL_ULTRA_THROTTLE.lock().await;
        let elapsed = last.elapsed();
        if elapsed < Duration::from_millis(180) {
            sleep(Duration::from_millis(180) - elapsed).await;
        }
        *last = Instant::now();
    }

    let url = format!(
        "{}?inputMint={}&outputMint={}&amount={}&taker={}&slippageBps={}",
        ULTRA_BASE_URL, input_mint, output_mint, amount, taker, slippage_bps
    );

    debug!(input_mint, output_mint, amount, "Ultra/Swap V2: fetching transaction");

    let mut attempt = 0u8;

    loop {
        let mut req = client.get(&url).header("accept", "application/json");
        if let Some(key) = api_key {
            req = req.header("x-api-key", key);
        }

        let resp = req.send().await?;
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();

        if status.as_u16() == 429 {
            attempt += 1;
            if attempt >= MAX_RETRIES {
                return Err(anyhow!("Ultra rate limit exceeded after {} attempts", attempt));
            }

            let base_delay = 400u64 * attempt as u64;
            let jitter = rand::random::<u64>() % 350;
            let delay = Duration::from_millis(base_delay + jitter);

            warn!(
                attempt,
                delay_ms = %delay.as_millis(),
                "Ultra 429 rate limit — retrying with backoff + jitter"
            );

            sleep(delay).await;
            continue;
        }

        if !status.is_success() {
            return Err(anyhow!("Ultra HTTP {}: {}", status, text));
        }

        // Parse JSON from text
        let raw: UltraOrderResponse = serde_json::from_str(&text)
            .map_err(|e| anyhow!("Failed to parse Ultra response: {}", e))?;

        let transaction_b64 = raw.transaction
            .filter(|s| !s.trim().is_empty())
            .ok_or_else(|| anyhow!("Ultra: transaction field empty"))?;

        let request_id = raw.request_id.unwrap_or_else(|| "unknown".to_string());

        let in_amount: u64 = raw.in_amount
            .and_then(|s| s.parse().ok())
            .unwrap_or(amount);

        let out_amount: u64 = raw.out_amount
            .and_then(|s| s.parse().ok())
            .ok_or_else(|| anyhow!("Ultra: out_amount missing"))?;

        let other_amount_threshold: u64 = raw.other_amount_threshold
            .and_then(|s| s.parse().ok())
            .unwrap_or(out_amount.saturating_mul(99) / 100);

        let price_impact_pct: f64 = raw.price_impact_pct
            .and_then(|s| s.parse().ok())
            .unwrap_or(0.0);

        let slippage_bps_resp = raw.slippage_bps.unwrap_or(slippage_bps as u64);

        let transaction_bytes = base64::engine::general_purpose::STANDARD
            .decode(&transaction_b64)
            .map_err(|e| anyhow!("base64 decode failed: {}", e))?;

        if transaction_bytes.is_empty() || transaction_bytes.len() < 100 {
            return Err(anyhow!("Ultra: decoded tx too small ({} bytes)", transaction_bytes.len()));
        }

        info!(
            input_mint = %input_mint,
            output_mint = %output_mint,
            in_amount,
            out_amount,
            price_impact_pct = format!("{:.4}%", price_impact_pct),
            tx_bytes = transaction_bytes.len(),
            "Ultra/Swap V2: real transaction fetched successfully"
        );

        return Ok(RouteExecutionData {
            input_mint: raw.input_mint.unwrap_or_else(|| input_mint.to_string()),
            output_mint: raw.output_mint.unwrap_or_else(|| output_mint.to_string()),
            in_amount,
            out_amount,
            other_amount_threshold,
            price_impact_pct,
            slippage_bps: slippage_bps_resp,
            transaction_b64,
            transaction_bytes,
            request_id,
            total_fee_lamports: 0,
        });
    }
}
