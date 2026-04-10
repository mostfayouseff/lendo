// =============================================================================
// JUPITER SWAP V2 — OPPORTUNITY DETECTION
//
// Endpoint: GET https://api.jup.ag/swap/v2/order
// Base URL:  https://api.jup.ag/swap/v2
// Auth:      x-api-key header (mandatory in production)
//
// Used ONLY for opportunity detection:
//   • Confirms a route exists at current market conditions
//   • Returns outAmount, priceImpact, routePlan for profitability analysis
//
// DO NOT use the returned data for MEV execution.
// For execution: use GET /build (swap_v2.rs) to get atomic swap instructions.
//
// FORBIDDEN endpoints (must not appear anywhere in this system):
//   /swap/v1/*  |  ultra/v1/*  |  /swap-instructions  |  lite-api.jup.ag
// =============================================================================

use anyhow::{anyhow, Result};
use reqwest::Client;
use serde::Deserialize;
use std::time::Duration;
use tracing::{info, warn};

pub const SWAP_V2_BASE: &str = "https://api.jup.ag/swap/v2";
const ORDER_TIMEOUT_MS: u64 = 6_000;

/// Maximum acceptable price impact — opportunities above this are rejected.
pub const MAX_PRICE_IMPACT_PCT: f64 = 1.5;

// ── API response ───────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct OrderResponse {
    pub input_mint:              Option<String>,
    pub output_mint:             Option<String>,
    pub in_amount:               Option<String>,
    pub out_amount:              Option<String>,
    pub other_amount_threshold:  Option<String>,
    pub swap_mode:               Option<String>,
    pub slippage_bps:            Option<u64>,
    pub price_impact_pct:        Option<String>,
    pub route_plan:              Option<Vec<OrderRoutePlanStep>>,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct OrderRoutePlanStep {
    pub swap_info: Option<OrderSwapInfo>,
    pub percent:   Option<u8>,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct OrderSwapInfo {
    pub amm_key:    Option<String>,
    pub label:      Option<String>,
    pub input_mint: Option<String>,
    pub output_mint:Option<String>,
    pub in_amount:  Option<String>,
    pub out_amount: Option<String>,
    pub fee_amount: Option<String>,
    pub fee_mint:   Option<String>,
}

// ── Detection result ──────────────────────────────────────────────────────────

/// Quote returned by GET /order — for profitability analysis only.
/// Never use this to construct a transaction for MEV execution.
#[derive(Debug, Clone)]
pub struct OpportunityQuote {
    pub input_mint:             String,
    pub output_mint:            String,
    pub in_amount:              u64,
    pub out_amount:             u64,
    pub other_amount_threshold: u64,
    pub price_impact_pct:       f64,
    pub slippage_bps:           u64,
    pub route_plan:             Vec<OrderRoutePlanStep>,
    pub total_fee_lamports:     u64,
}

impl OpportunityQuote {
    /// True when the opportunity meets the minimum profit threshold and price
    /// impact is within acceptable bounds.
    pub fn is_profitable(&self, min_profit_lamports: u64) -> bool {
        let profit = self.expected_profit_lamports();
        profit >= 0
            && profit as u64 >= min_profit_lamports
            && self.price_impact_pct <= MAX_PRICE_IMPACT_PCT
    }

    /// Expected profit after fees (may be negative).
    pub fn expected_profit_lamports(&self) -> i64 {
        self.out_amount as i64
            - self.in_amount as i64
            - self.total_fee_lamports as i64
    }
}

// ── Client builder ─────────────────────────────────────────────────────────────

/// Build an HTTP client suitable for Jupiter Swap V2 API calls.
pub fn build_swap_v2_client() -> Result<Client> {
    Ok(Client::builder()
        .timeout(Duration::from_millis(ORDER_TIMEOUT_MS))
        .build()?)
}

// ── Detection ──────────────────────────────────────────────────────────────────

/// Detect an arbitrage opportunity by querying Jupiter Swap V2 GET /order.
///
/// This is STRICTLY for opportunity detection — the returned data must never
/// be used to construct a transaction.  For execution, call
/// `get_build_instructions` (GET /build).
///
/// If the API returns an error the caller MUST skip this loop iteration.
pub async fn detect_opportunity(
    client: &Client,
    input_mint:  &str,
    output_mint: &str,
    amount:      u64,
    slippage_bps: u16,
    api_key: Option<&str>,
) -> Result<OpportunityQuote> {
    if input_mint == output_mint {
        return Err(anyhow!("detect_opportunity: input_mint == output_mint"));
    }
    if amount == 0 {
        return Err(anyhow!("detect_opportunity: amount must be > 0"));
    }

    let url = format!(
        "{}/order?inputMint={}&outputMint={}&amount={}&slippageBps={}&onlyDirectRoutes=false",
        SWAP_V2_BASE, input_mint, output_mint, amount, slippage_bps
    );

    info!(
        url          = %url,
        input_mint,
        output_mint,
        amount,
        slippage_bps,
        has_api_key  = api_key.is_some(),
        "► SENDING GET /swap/v2/order — opportunity detection request"
    );

    let mut req = client.get(&url).header("accept", "application/json");
    if let Some(key) = api_key {
        req = req.header("x-api-key", key);
    }

    let resp = req.send().await?;
    let status = resp.status();

    info!(
        status = %status,
        url    = %url,
        "◄ RESPONSE GET /swap/v2/order — HTTP status received"
    );

    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(anyhow!("GET /order HTTP {}: {}", status, &body[..body.len().min(400)]));
    }

    let raw: OrderResponse = resp.json().await
        .map_err(|e| anyhow!("GET /order parse failed: {e}"))?;

    let in_amount: u64 = raw.in_amount.as_deref()
        .and_then(|s| s.parse().ok())
        .unwrap_or(amount);

    let out_amount: u64 = raw.out_amount.as_deref()
        .and_then(|s| s.parse().ok())
        .ok_or_else(|| anyhow!("GET /order: outAmount missing or invalid"))?;

    let other_amount_threshold: u64 = raw.other_amount_threshold.as_deref()
        .and_then(|s| s.parse().ok())
        .unwrap_or_else(|| out_amount.saturating_mul(99) / 100);

    let price_impact_pct: f64 = raw.price_impact_pct.as_deref()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0.0);

    let slippage_bps_resp = raw.slippage_bps.unwrap_or(slippage_bps as u64);

    let route_plan = raw.route_plan.unwrap_or_default();

    let total_fee_lamports: u64 = route_plan.iter()
        .filter_map(|s| s.swap_info.as_ref())
        .filter_map(|si| si.fee_amount.as_deref())
        .filter_map(|s| s.parse::<u64>().ok())
        .sum();

    let input_mint_resolved  = raw.input_mint.unwrap_or_else(|| input_mint.to_string());
    let output_mint_resolved = raw.output_mint.unwrap_or_else(|| output_mint.to_string());

    let quote = OpportunityQuote {
        input_mint:  input_mint_resolved.clone(),
        output_mint: output_mint_resolved.clone(),
        in_amount,
        out_amount,
        other_amount_threshold,
        price_impact_pct,
        slippage_bps: slippage_bps_resp,
        route_plan,
        total_fee_lamports,
    };

    info!(
        input_mint   = %input_mint_resolved,
        output_mint  = %output_mint_resolved,
        in_amount,
        out_amount,
        price_impact = format!("{:.4}%", price_impact_pct),
        fee_lamports = total_fee_lamports,
        profit       = quote.expected_profit_lamports(),
        "GET /order: opportunity quote received"
    );

    if price_impact_pct > MAX_PRICE_IMPACT_PCT {
        warn!(
            price_impact_pct = format!("{:.4}%", price_impact_pct),
            threshold        = MAX_PRICE_IMPACT_PCT,
            "GET /order: price impact exceeds threshold — opportunity will be rejected"
        );
    }

    Ok(quote)
}
