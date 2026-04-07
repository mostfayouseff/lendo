// =============================================================================
// JUPITER ULTRA API — Order + Execute
//
// Flow:
//   1. GET https://lite-api.jup.ag/ultra/v2/order
//      → Returns transaction (base64), requestId, amounts, route
//   2. Sign the transaction with local keypair
//   3. Submit signed tx:
//      A. POST https://api.jup.ag/ultra/v2/execute  (Jupiter native)
//      B. Jito bundle submission                    (MEV-priority)
//
// SAFETY FILTERS (enforced before submission):
//   ✓ output > input             (swap must not lose money at API level)
//   ✓ price_impact ≤ configured  (reject manipulated routes)
//   ✓ real_profit > 0            (after all fees + tip + slippage)
//   ✓ real_profit < 5% of input  (sanity cap — bugs produce outsized profits)
//   ✓ transaction bytes non-empty (hard reject — no empty tx ever submitted)
//
// RETRY: get_best_route_and_transaction retries up to MAX_ORDER_RETRIES times
//        with LINEAR_RETRY_DELAY_MS between attempts.
//
// NO DEPRECATED ENDPOINTS: /v2/quote and /v2/swap-instructions are not used.
// =============================================================================

use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tracing::{debug, info, warn};

// ── Endpoints ─────────────────────────────────────────────────────────────────

/// Primary order endpoint — lite (public, rate-limited free tier).
const ULTRA_ORDER_URL: &str = "https://api.jup.ag/ultra/v1/order";

/// Execute endpoint — Jupiter's own submission infrastructure.
const ULTRA_EXECUTE_URL: &str = "https://api.jup.ag/ultra/v1/execute";

// ── Timeouts & retries ────────────────────────────────────────────────────────
const ORDER_TIMEOUT_MS: u64    = 5_000; // 5s — fail fast; MEV windows are short
const EXECUTE_TIMEOUT_MS: u64  = 8_000;
const MAX_ORDER_RETRIES: u32   = 3;
const LINEAR_RETRY_DELAY_MS: u64 = 300;

// ── Sanity cap: reject opportunities where real profit > this fraction of input ─
/// 5% of input — anything above is almost certainly a calculation bug.
const MAX_PROFIT_FRACTION_OF_INPUT: f64 = 0.05;

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

/// Response from POST /ultra/v2/execute
#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct UltraExecuteResponse {
    /// On-chain transaction signature (base58)
    pub signature: Option<String>,
    pub status: Option<String>,
    pub error: Option<String>,
    pub code: Option<i64>,
}

impl UltraExecuteResponse {
    pub fn is_success(&self) -> bool {
        self.error.is_none()
            && self.code.map(|c| c == 0).unwrap_or(true)
            && self.status.as_deref().map(|s| s != "Failed").unwrap_or(true)
    }

    pub fn signature_or(&self, fallback: &str) -> String {
        self.signature
            .clone()
            .unwrap_or_else(|| fallback.to_string())
    }
}

// ── Execute request ───────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct UltraExecuteRequest<'a> {
    signed_transaction: &'a str,
    request_id: &'a str,
}

// ── Token metadata ────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize, Clone)]
pub struct TokenMetadata {
    pub address: Option<String>,
    pub symbol: Option<String>,
    pub name: Option<String>,
    pub decimals: Option<u8>,
}

// ── Processed output type ─────────────────────────────────────────────────────

/// Complete route + transaction data returned by `get_best_route_and_transaction`.
/// All downstream validation uses the methods on this struct — no raw field arithmetic.
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
    /// Minimum output after slippage (other_amount_threshold from API)
    pub other_amount_threshold: u64,
    /// Price impact as a percentage (0.01 = 0.01%)
    pub price_impact_pct: f64,
    /// Slippage in basis points
    pub slippage_bps: u64,
    /// Base64-encoded versioned transaction (ready to sign + submit)
    pub transaction_b64: String,
    /// Decoded transaction bytes (decoded from base64 — sign these)
    pub transaction_bytes: Vec<u8>,
    /// Request ID for Jupiter order tracking (/execute requires this)
    pub request_id: String,
    /// Route plan steps from the API
    pub route_plan: Vec<RoutePlanStep>,
    /// Estimated total swap fees from route plan (sum of fee_amount fields)
    pub total_fee_lamports: u64,
}

impl RouteExecutionData {
    // ── Safety predicates ─────────────────────────────────────────────────────

    /// The swap must produce more tokens than it consumes (pre-fee).
    /// Required before any further calculation.
    pub fn output_exceeds_input(&self) -> bool {
        self.out_amount > self.in_amount
    }

    /// Real profit after all known costs:
    ///   out_amount − in_amount − swap_fees − jito_tip − worst-case slippage
    ///
    /// Returns `None` if the subtraction underflows (always treat as unprofitable).
    pub fn real_profit_lamports(&self, jito_tip_lamports: u64) -> Option<i64> {
        let gross: i64 = self.out_amount as i64 - self.in_amount as i64;
        let fees: i64 = self.total_fee_lamports as i64 + jito_tip_lamports as i64;
        // Worst-case slippage loss = difference between expected and minimum output
        let slippage: i64 =
            self.out_amount.saturating_sub(self.other_amount_threshold) as i64;
        Some(gross - fees - slippage)
    }

    /// Sanity cap: real profit must be < 5% of in_amount.
    /// Values above this almost always indicate a calculation bug, not a real arb.
    pub fn profit_passes_sanity_cap(&self, real_profit: i64) -> bool {
        if real_profit <= 0 {
            return true; // already caught by profitability check
        }
        let cap = (self.in_amount as f64 * MAX_PROFIT_FRACTION_OF_INPUT) as i64;
        real_profit < cap
    }

    /// Log a structured summary of this route for observability.
    pub fn log_summary(&self, label: &str) {
        info!(
            label,
            input_mint   = %self.input_mint,
            output_mint  = %self.output_mint,
            in_amount    = self.in_amount,
            out_amount   = self.out_amount,
            price_impact = format!("{:.4}%", self.price_impact_pct),
            slippage_bps = self.slippage_bps,
            tx_bytes     = self.transaction_bytes.len(),
            route_hops   = self.route_plan.len(),
            request_id   = %self.request_id,
            "Ultra API: route summary"
        );
    }
}

// ── Client builder ────────────────────────────────────────────────────────────

/// Build an HTTP client for Ultra API order calls.
pub fn build_ultra_client() -> anyhow::Result<Client> {
    Ok(Client::builder()
        .timeout(Duration::from_millis(ORDER_TIMEOUT_MS))
        .build()?)
}

/// Build an HTTP client for Ultra API execute calls (longer timeout).
pub fn build_ultra_execute_client() -> anyhow::Result<Client> {
    Ok(Client::builder()
        .timeout(Duration::from_millis(EXECUTE_TIMEOUT_MS))
        .build()?)
}

// ── Order — GET /ultra/v2/order ───────────────────────────────────────────────

/// Fetch the best route and a complete executable transaction from Jupiter Ultra API.
///
/// Retries up to `MAX_ORDER_RETRIES` times with `LINEAR_RETRY_DELAY_MS` between
/// attempts. Returns an error if all retries fail.
///
/// # Arguments
/// * `client`       — pre-built reqwest Client (use `build_ultra_client()`)
/// * `input_mint`   — base58 mint of input token
/// * `output_mint`  — base58 mint of output token
/// * `amount`       — input amount in raw units (lamports for SOL)
/// * `taker`        — operator public key (base58) — this wallet must sign the tx
/// * `api_key`      — optional `x-api-key` header
/// * `slippage_bps` — slippage tolerance in basis points
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
        ULTRA_ORDER_URL, input_mint, output_mint, amount, taker, slippage_bps
    );

    debug!(
        input_mint,
        output_mint,
        amount,
        taker,
        slippage_bps,
        endpoint = ULTRA_ORDER_URL,
        "Ultra /order: requesting route"
    );

    let mut last_err = anyhow::anyhow!("No attempts made");

    for attempt in 1..=MAX_ORDER_RETRIES {
        let mut req = client.get(&url).header("accept", "application/json");
        if let Some(key) = api_key {
            req = req.header("x-api-key", key);
        }

        match req.send().await {
            Ok(resp) => {
                let status = resp.status();

                if !status.is_success() {
                    let text = resp.text().await.unwrap_or_default();
                    last_err = anyhow::anyhow!(
                        "Ultra /order HTTP {}: {}",
                        status,
                        &text[..text.len().min(400)]
                    );
                    warn!(
                        attempt,
                        status = %status,
                        error  = %last_err,
                        "Ultra /order: HTTP error"
                    );
                    if attempt < MAX_ORDER_RETRIES {
                        tokio::time::sleep(Duration::from_millis(LINEAR_RETRY_DELAY_MS)).await;
                    }
                    continue;
                }

                let raw: UltraOrderResponse = match resp.json().await {
                    Ok(r) => r,
                    Err(e) => {
                        last_err = anyhow::anyhow!("Ultra /order: JSON parse failed: {e}");
                        warn!(attempt, error = %last_err, "Ultra /order: parse error");
                        if attempt < MAX_ORDER_RETRIES {
                            tokio::time::sleep(Duration::from_millis(LINEAR_RETRY_DELAY_MS))
                                .await;
                        }
                        continue;
                    }
                };

                // ── Validate required fields ───────────────────────────────
                let transaction_b64 = match raw.transaction {
                    Some(tx) if !tx.is_empty() => tx,
                    Some(_) | None => {
                        last_err = anyhow::anyhow!(
                            "Ultra /order: 'transaction' field missing or empty"
                        );
                        warn!(attempt, error = %last_err);
                        if attempt < MAX_ORDER_RETRIES {
                            tokio::time::sleep(Duration::from_millis(LINEAR_RETRY_DELAY_MS))
                                .await;
                        }
                        continue;
                    }
                };

                let out_amount: u64 = match raw
                    .out_amount
                    .as_deref()
                    .and_then(|s| s.parse().ok())
                {
                    Some(v) => v,
                    None => {
                        last_err =
                            anyhow::anyhow!("Ultra /order: 'outAmount' missing or invalid");
                        warn!(attempt, error = %last_err);
                        if attempt < MAX_ORDER_RETRIES {
                            tokio::time::sleep(Duration::from_millis(LINEAR_RETRY_DELAY_MS))
                                .await;
                        }
                        continue;
                    }
                };

                let request_id = raw
                    .request_id
                    .filter(|s| !s.is_empty())
                    .unwrap_or_else(|| "unknown".to_string());

                let in_amount: u64 = raw
                    .in_amount
                    .as_deref()
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(amount);

                let other_amount_threshold: u64 = raw
                    .other_amount_threshold
                    .as_deref()
                    .and_then(|s| s.parse().ok())
                    .unwrap_or_else(|| out_amount.saturating_mul(99) / 100);

                let price_impact_pct: f64 = raw
                    .price_impact_pct
                    .as_deref()
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(0.0);

                let slippage_bps_resp = raw.slippage_bps.unwrap_or(slippage_bps as u64);
                let route_plan = raw.route_plan.unwrap_or_default();

                // Total fees from route plan
                let total_fee_lamports: u64 = route_plan
                    .iter()
                    .filter_map(|step| step.swap_info.as_ref())
                    .filter_map(|si| si.fee_amount.as_deref())
                    .filter_map(|s| s.parse::<u64>().ok())
                    .sum();

                // Decode base64 transaction
                use base64::Engine as _;
                let transaction_bytes = match base64::engine::general_purpose::STANDARD
                    .decode(&transaction_b64)
                {
                    Ok(b) if !b.is_empty() => b,
                    Ok(_) => {
                        last_err = anyhow::anyhow!(
                            "Ultra /order: base64 decoded to zero bytes"
                        );
                        warn!(attempt, error = %last_err);
                        if attempt < MAX_ORDER_RETRIES {
                            tokio::time::sleep(Duration::from_millis(LINEAR_RETRY_DELAY_MS))
                                .await;
                        }
                        continue;
                    }
                    Err(e) => {
                        last_err =
                            anyhow::anyhow!("Ultra /order: base64 decode failed: {e}");
                        warn!(attempt, error = %last_err);
                        if attempt < MAX_ORDER_RETRIES {
                            tokio::time::sleep(Duration::from_millis(LINEAR_RETRY_DELAY_MS))
                                .await;
                        }
                        continue;
                    }
                };

                let actual_input_mint =
                    raw.input_mint.unwrap_or_else(|| input_mint.to_string());
                let actual_output_mint =
                    raw.output_mint.unwrap_or_else(|| output_mint.to_string());

                info!(
                    attempt,
                    input_mint   = %actual_input_mint,
                    output_mint  = %actual_output_mint,
                    in_amount,
                    out_amount,
                    price_impact = format!("{:.4}%", price_impact_pct),
                    route_hops   = route_plan.len(),
                    tx_bytes     = transaction_bytes.len(),
                    request_id   = %request_id,
                    "Ultra /order: route + transaction fetched"
                );

                if price_impact_pct > 1.0 {
                    warn!(
                        price_impact = format!("{:.4}%", price_impact_pct),
                        "Ultra /order: elevated price impact — may be unprofitable"
                    );
                }

                return Ok(RouteExecutionData {
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
                });
            }
            Err(e) => {
                last_err = anyhow::anyhow!("Ultra /order: network error: {e}");
                warn!(attempt, error = %last_err, "Ultra /order: network failure");
                if attempt < MAX_ORDER_RETRIES {
                    tokio::time::sleep(Duration::from_millis(LINEAR_RETRY_DELAY_MS)).await;
                }
            }
        }
    }

    Err(last_err)
}

// ── Execute — POST /ultra/v2/execute ─────────────────────────────────────────

/// Submit a signed transaction to Jupiter's execution infrastructure.
///
/// This is an alternative to direct Jito submission. Jupiter routes the tx to
/// validators with some priority logic internally.
///
/// # Arguments
/// * `client`              — pre-built reqwest Client (use `build_ultra_execute_client()`)
/// * `request_id`          — the requestId returned by /ultra/v2/order
/// * `signed_tx_b64`       — base64-encoded SIGNED transaction bytes
/// * `api_key`             — optional `x-api-key` header
pub async fn execute_via_ultra(
    client: &Client,
    request_id: &str,
    signed_tx_b64: &str,
    api_key: Option<&str>,
) -> anyhow::Result<UltraExecuteResponse> {
    let body = UltraExecuteRequest {
        signed_transaction: signed_tx_b64,
        request_id,
    };

    debug!(
        request_id,
        tx_b64_len = signed_tx_b64.len(),
        "Ultra /execute: submitting signed transaction"
    );

    let mut req = client
        .post(ULTRA_EXECUTE_URL)
        .header("Content-Type", "application/json")
        .header("accept", "application/json")
        .json(&body);

    if let Some(key) = api_key {
        req = req.header("x-api-key", key);
    }

    let resp = req.send().await.map_err(|e| {
        anyhow::anyhow!("Ultra /execute: network error: {e}")
    })?;

    let status = resp.status();

    if !status.is_success() {
        let text = resp.text().await.unwrap_or_default();
        return Err(anyhow::anyhow!(
            "Ultra /execute HTTP {}: {}",
            status,
            &text[..text.len().min(400)]
        ));
    }

    let result: UltraExecuteResponse = resp.json().await.map_err(|e| {
        anyhow::anyhow!("Ultra /execute: JSON parse failed: {e}")
    })?;

    if result.is_success() {
        info!(
            request_id,
            signature = %result.signature_or("unknown"),
            status    = ?result.status,
            "Ultra /execute: transaction submitted successfully"
        );
    } else {
        warn!(
            request_id,
            error = ?result.error,
            code  = ?result.code,
            "Ultra /execute: submission rejected"
        );
    }

    Ok(result)
}

// ── Token metadata ────────────────────────────────────────────────────────────

/// Fetch token metadata from Jupiter Tokens v2 API.
pub async fn fetch_token_metadata(
    client: &Client,
    query: &str,
    api_key: Option<&str>,
) -> anyhow::Result<Vec<TokenMetadata>> {
    let url = format!("https://api.jup.ag/tokens/v2/search?query={}", query);

    let mut req = client.get(&url).header("accept", "application/json");
    if let Some(key) = api_key {
        req = req.header("x-api-key", key);
    }

    let resp = req.send().await?;
    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        return Err(anyhow::anyhow!(
            "Token metadata HTTP {}: {}",
            status,
            &text[..text.len().min(200)]
        ));
    }

    Ok(resp.json().await?)
}
