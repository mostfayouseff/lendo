// =============================================================================
// JUPITER ULTRA API — Real-time Route + Executable Transaction Fetcher
//
// Endpoint: https://api.jup.ag/ultra/v1/order
//
// Returns a complete, signed transaction (base64) ready for on-chain execution.
// The `taker` field must be the operator's public key — they sign the tx.
//
// STRUCTS:
//   RouteExecutionData — output of get_best_route_and_transaction()
//   UltraOrderResponse — raw API deserialization target
//
// USAGE:
//   let data = get_best_route_and_transaction(
//       &client, input_mint, output_mint, amount_lamports, taker_pubkey,
//       api_key, slippage_bps,
//   ).await?;
//   // data.transaction is a base64-encoded versioned transaction
//   // Sign with operator keypair and submit to Jito
// =============================================================================

use reqwest::Client;
use serde::Deserialize;
use std::time::Duration;
use tracing::{debug, info, warn};

const ULTRA_TIMEOUT_MS: u64 = 8_000;
const ULTRA_BASE_URL: &str = "https://api.jup.ag/ultra/v1/order";

// ── API response types ────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UltraOrderResponse {
    pub input_mint: Option<String>,
    pub output_mint: Option<String>,
    pub in_amount: Option<String>,
    pub out_amount: Option<String>,
    pub other_amount_threshold: Option<String>,
    pub swap_mode: Option<String>,
    pub slippage_bps: Option<u64>,
    pub price_impact_pct: Option<String>,
    pub route_plan: Option<Vec<RoutePlanStep>>,
    pub transaction: Option<String>,
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
    pub amm_key: Option<String>,
    pub label: Option<String>,
    pub input_mint: Option<String>,
    pub output_mint: Option<String>,
    pub in_amount: Option<String>,
    pub out_amount: Option<String>,
    pub fee_amount: Option<String>,
    pub fee_mint: Option<String>,
}

// ── Processed output type ─────────────────────────────────────────────────────

/// Complete route + transaction data returned by the Ultra API.
#[derive(Debug, Clone)]
pub struct RouteExecutionData {
    /// Input token mint address (base58)
    pub input_mint: String,
    /// Output token mint address (base58)
    pub output_mint: String,
    /// Input amount in raw token units (lamports for SOL)
    pub in_amount: u64,
    /// Expected output amount in raw token units
    pub out_amount: u64,
    /// Minimum output after slippage (other_amount_threshold)
    pub other_amount_threshold: u64,
    /// Price impact as a percentage (0.01 = 0.01%)
    pub price_impact_pct: f64,
    /// Slippage in basis points
    pub slippage_bps: u64,
    /// Base64-encoded versioned transaction (ready to sign + submit)
    pub transaction_b64: String,
    /// Decoded transaction bytes (decoded from base64)
    pub transaction_bytes: Vec<u8>,
    /// Request ID for Jupiter order tracking
    pub request_id: String,
    /// Route plan steps
    pub route_plan: Vec<RoutePlanStep>,
    /// Fee amounts per hop in lamports
    pub total_fee_lamports: u64,
}

impl RouteExecutionData {
    /// Calculate expected profit if this swap closes a circular arb.
    /// For a single swap (A→B), profit is only known when we also have B→A quote.
    /// For a single direction: net = out_amount - in_amount (same denomination assumed).
    pub fn expected_profit_lamports_vs(&self, original_input: u64) -> i64 {
        self.out_amount as i64 - original_input as i64 - self.total_fee_lamports as i64
    }

    /// Whether the swap is profitable vs. raw in_amount (out > in after fees).
    pub fn is_profitable(&self) -> bool {
        self.out_amount > self.in_amount.saturating_add(self.total_fee_lamports)
    }
}

// ── Ultra API client ──────────────────────────────────────────────────────────

/// Build an HTTP client configured for Ultra API calls.
pub fn build_ultra_client() -> anyhow::Result<Client> {
    Ok(Client::builder()
        .timeout(Duration::from_millis(ULTRA_TIMEOUT_MS))
        .build()?)
}

/// Fetch the best route and a complete executable transaction from Jupiter Ultra API.
///
/// # Arguments
/// * `client`        — pre-built reqwest Client
/// * `input_mint`    — base58 mint of input token (e.g. SOL)
/// * `output_mint`   — base58 mint of output token (e.g. USDC)
/// * `amount`        — input amount in raw token units (lamports for SOL)
/// * `taker`         — operator public key (base58) — this wallet signs the tx
/// * `api_key`       — optional Jupiter API key (x-api-key header)
/// * `slippage_bps`  — slippage tolerance in basis points
///
/// # Returns
/// `RouteExecutionData` with a ready-to-sign transaction, or an error if the API
/// fails or the route is not available.
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

    debug!(
        input_mint,
        output_mint,
        amount,
        taker,
        slippage_bps,
        "Ultra API: fetching route and transaction"
    );

    let mut req = client
        .get(&url)
        .header("accept", "application/json");

    if let Some(key) = api_key {
        req = req.header("x-api-key", key);
    }

    let resp = req.send().await?;
    let status = resp.status();

    if !status.is_success() {
        let text = resp.text().await.unwrap_or_default();
        return Err(anyhow::anyhow!(
            "Ultra API HTTP {}: {}",
            status,
            &text[..text.len().min(400)]
        ));
    }

    let raw: UltraOrderResponse = resp.json().await.map_err(|e| {
        anyhow::anyhow!("Ultra API response parse failed: {e}")
    })?;

    // Validate required fields
    let transaction_b64 = raw.transaction.ok_or_else(|| {
        anyhow::anyhow!("Ultra API: 'transaction' field missing from response")
    })?;
    let request_id = raw.request_id.unwrap_or_else(|| "unknown".to_string());

    let in_amount: u64 = raw
        .in_amount
        .as_deref()
        .and_then(|s| s.parse().ok())
        .unwrap_or(amount);

    let out_amount: u64 = raw
        .out_amount
        .as_deref()
        .and_then(|s| s.parse().ok())
        .ok_or_else(|| anyhow::anyhow!("Ultra API: 'outAmount' missing or invalid"))?;

    let other_amount_threshold: u64 = raw
        .other_amount_threshold
        .as_deref()
        .and_then(|s| s.parse().ok())
        .unwrap_or(out_amount.saturating_mul(99) / 100);

    let price_impact_pct: f64 = raw
        .price_impact_pct
        .as_deref()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0.0);

    let slippage_bps_resp = raw.slippage_bps.unwrap_or(slippage_bps as u64);

    let route_plan = raw.route_plan.unwrap_or_default();

    // Estimate total fees from route plan
    let total_fee_lamports: u64 = route_plan
        .iter()
        .filter_map(|step| step.swap_info.as_ref())
        .filter_map(|si| si.fee_amount.as_deref())
        .filter_map(|s| s.parse::<u64>().ok())
        .sum();

    // Decode the transaction from base64
    use base64::Engine as _;
    let transaction_bytes = base64::engine::general_purpose::STANDARD
        .decode(&transaction_b64)
        .map_err(|e| anyhow::anyhow!("Ultra API: base64 decode failed: {e}"))?;

    let actual_input_mint = raw.input_mint.unwrap_or_else(|| input_mint.to_string());
    let actual_output_mint = raw.output_mint.unwrap_or_else(|| output_mint.to_string());

    info!(
        input_mint  = %actual_input_mint,
        output_mint = %actual_output_mint,
        in_amount,
        out_amount,
        price_impact_pct = format!("{:.4}%", price_impact_pct),
        slippage_bps     = slippage_bps_resp,
        tx_bytes         = transaction_bytes.len(),
        route_hops       = route_plan.len(),
        "Ultra API: route + transaction fetched successfully"
    );

    if price_impact_pct > 1.0 {
        warn!(
            price_impact_pct = format!("{:.4}%", price_impact_pct),
            "Ultra API: high price impact — trade may be unprofitable"
        );
    }

    Ok(RouteExecutionData {
        input_mint: actual_input_mint,
        output_mint: actual_output_mint,
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

/// Fetch token metadata from Jupiter Tokens v2 API.
/// Returns a map of symbol → (mint, decimals).
pub async fn fetch_token_metadata(
    client: &Client,
    query: &str,
    api_key: Option<&str>,
) -> anyhow::Result<Vec<TokenMetadata>> {
    let url = format!("https://api.jup.ag/tokens/v2/search?query={}", query);

    let mut req = client
        .get(&url)
        .header("accept", "application/json");

    if let Some(key) = api_key {
        req = req.header("x-api-key", key);
    }

    let resp = req.send().await?;
    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        return Err(anyhow::anyhow!("Token metadata HTTP {}: {}", status, &text[..text.len().min(200)]));
    }

    let tokens: Vec<TokenMetadata> = resp.json().await?;
    Ok(tokens)
}

#[derive(Debug, Deserialize, Clone)]
pub struct TokenMetadata {
    pub address: Option<String>,
    pub symbol: Option<String>,
    pub name: Option<String>,
    pub decimals: Option<u8>,
}
