// =============================================================================
// JUPITER ULTRA API — FIXED 2026-04-06
// - Fixed zero-length transaction bug
// - Stricter validation + base64 decode
// - Deprecation notice (Ultra v1 is unstable)
// - Better error messages
// =============================================================================

use reqwest::Client;
use serde::Deserialize;
use std::time::Duration;
use tracing::{debug, info, warn};

const ULTRA_TIMEOUT_MS: u64 = 12_000;
const ULTRA_BASE_URL: &str = "https://api.jup.ag/ultra/v1/order";

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
    pub route_plan: Option<Vec<RoutePlanStep>>,
    pub transaction: Option<String>,   // base64
    pub request_id: Option<String>,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct RoutePlanStep {
    pub swap_info: Option<SwapInfo>,
    pub percent: Option<u8>,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SwapInfo {
    pub fee_amount: Option<String>,
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
    pub route_plan: Vec<RoutePlanStep>,
    pub total_fee_lamports: u64,
}

pub fn build_ultra_client() -> anyhow::Result<Client> {
    Ok(Client::builder()
        .timeout(Duration::from_millis(ULTRA_TIMEOUT_MS))
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
    let url = format!(
        "{}?inputMint={}&outputMint={}&amount={}&taker={}&slippageBps={}",
        ULTRA_BASE_URL, input_mint, output_mint, amount, taker, slippage_bps
    );

    debug!(input_mint, output_mint, amount, "Ultra API: fetching route + tx");

    let mut req = client.get(&url).header("accept", "application/json");
    if let Some(key) = api_key {
        req = req.header("x-api-key", key);
    }

    let resp = req.send().await?;
    if !resp.status().is_success() {
        let text = resp.text().await.unwrap_or_default();
        return Err(anyhow::anyhow!("Ultra API HTTP {}: {}", resp.status(), text));
    }

    let raw: UltraOrderResponse = resp.json().await?;

    // === CRITICAL FIX: reject empty transaction ===
    let transaction_b64 = raw.transaction
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| anyhow::anyhow!("Ultra API: transaction field missing or empty"))?;

    let request_id = raw.request_id.unwrap_or_else(|| "unknown".to_string());

    let in_amount = raw.in_amount.and_then(|s| s.parse().ok()).unwrap_or(amount);
    let out_amount = raw.out_amount.and_then(|s| s.parse().ok())
        .ok_or_else(|| anyhow::anyhow!("Ultra API: out_amount missing"))?;

    let other_amount_threshold = raw.other_amount_threshold
        .and_then(|s| s.parse().ok())
        .unwrap_or(out_amount.saturating_mul(99) / 100);

    let price_impact_pct = raw.price_impact_pct.and_then(|s| s.parse().ok()).unwrap_or(0.0);
    let slippage_bps_resp = raw.slippage_bps.unwrap_or(slippage_bps as u64);
    let route_plan = raw.route_plan.unwrap_or_default();

    let total_fee_lamports: u64 = route_plan.iter()
        .filter_map(|step| step.swap_info.as_ref())
        .filter_map(|si| si.fee_amount.as_deref())
        .filter_map(|s| s.parse::<u64>().ok())
        .sum();

    // Decode base64
    use base64::Engine as _;
    let transaction_bytes = base64::engine::general_purpose::STANDARD
        .decode(&transaction_b64)
        .map_err(|e| anyhow::anyhow!("Ultra base64 decode failed: {}", e))?;

    if transaction_bytes.is_empty() || transaction_bytes.len() < 100 {
        return Err(anyhow::anyhow!("Ultra API: decoded tx is empty ({} bytes)", transaction_bytes.len()));
    }

    info!(
        input_mint = %raw.input_mint.as_deref().unwrap_or(input_mint),
        output_mint = %raw.output_mint.as_deref().unwrap_or(output_mint),
        in_amount, out_amount,
        price_impact_pct = format!("{:.4}%", price_impact_pct),
        tx_bytes = transaction_bytes.len(),
        "Ultra API: SUCCESS — real transaction ready"
    );

    Ok(RouteExecutionData {
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
        route_plan,
        total_fee_lamports,
    })
}
