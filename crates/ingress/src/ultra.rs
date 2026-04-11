use anyhow::{anyhow, Result};
use reqwest::{Client, StatusCode};
use serde::Deserialize;
use std::time::Duration;
use tracing::{info, warn};

pub const JUPITER_ULTRA_BASE: &str = "https://api.jup.ag/ultra/v1";
const ULTRA_TIMEOUT_MS: u64 = 8_000;
const MAX_QUOTE_ATTEMPTS: u32 = 2;

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct UltraRoutePlanStep {
    pub swap_info: Option<UltraSwapInfo>,
    pub percent: Option<u8>,
    pub bps: Option<u64>,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct UltraSwapInfo {
    pub amm_key: Option<String>,
    pub label: Option<String>,
    pub input_mint: Option<String>,
    pub output_mint: Option<String>,
    pub in_amount: Option<String>,
    pub out_amount: Option<String>,
    pub fee_amount: Option<String>,
    pub fee_mint: Option<String>,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct UltraOrderResponse {
    pub mode: Option<String>,
    pub input_mint: Option<String>,
    pub output_mint: Option<String>,
    pub in_amount: Option<String>,
    pub out_amount: Option<String>,
    pub other_amount_threshold: Option<String>,
    pub swap_mode: Option<String>,
    pub slippage_bps: Option<u64>,
    pub price_impact: Option<f64>,
    pub price_impact_pct: Option<String>,
    pub route_plan: Option<Vec<UltraRoutePlanStep>>,
    pub signature_fee_lamports: Option<u64>,
    pub prioritization_fee_lamports: Option<u64>,
    pub rent_fee_lamports: Option<u64>,
    pub request_id: Option<String>,
    pub transaction: Option<String>,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
}

#[derive(Debug, Clone)]
pub struct UltraOrder {
    pub input_mint: String,
    pub output_mint: String,
    pub in_amount: u64,
    pub out_amount: u64,
    pub price_impact_pct: f64,
    pub route_plan: Vec<UltraRoutePlanStep>,
    pub signature_fee_lamports: u64,
    pub prioritization_fee_lamports: u64,
    pub rent_fee_lamports: u64,
    pub request_id: String,
    pub transaction_b64: String,
}

impl UltraOrder {
    pub fn expected_profit_lamports(&self) -> i64 {
        self.out_amount as i64
            - self.in_amount as i64
            - self.signature_fee_lamports as i64
            - self.prioritization_fee_lamports as i64
            - self.rent_fee_lamports as i64
    }

    pub fn required_fee_lamports(&self) -> u64 {
        self.signature_fee_lamports
            .saturating_add(self.prioritization_fee_lamports)
            .saturating_add(self.rent_fee_lamports)
    }
}

pub fn build_ultra_client() -> Result<Client> {
    Ok(Client::builder()
        .timeout(Duration::from_millis(ULTRA_TIMEOUT_MS))
        .build()?)
}

pub async fn request_ultra_order(
    client: &Client,
    input_mint: &str,
    output_mint: &str,
    amount: u64,
    taker: &str,
    api_key: &str,
) -> Result<UltraOrder> {
    if input_mint == output_mint {
        return Err(anyhow!("Ultra order rejected: input_mint == output_mint"));
    }
    if amount == 0 {
        return Err(anyhow!("Ultra order rejected: amount must be > 0"));
    }
    if taker.is_empty() {
        return Err(anyhow!("Ultra order rejected: taker wallet missing"));
    }

    let url = format!(
        "{}/order?inputMint={}&outputMint={}&amount={}&taker={}",
        JUPITER_ULTRA_BASE, input_mint, output_mint, amount, taker
    );

    let mut last_error: Option<anyhow::Error> = None;
    for attempt in 1..=MAX_QUOTE_ATTEMPTS {
        info!(
            stage = "[QUOTE REQUEST]",
            attempt,
            max_attempts = MAX_QUOTE_ATTEMPTS,
            url = %url,
            input_mint,
            output_mint,
            amount,
            taker,
            "Requesting Jupiter Ultra order"
        );

        let resp = client
            .get(&url)
            .header("accept", "application/json")
            .header("x-api-key", api_key)
            .send()
            .await;

        let resp = match resp {
            Ok(resp) => resp,
            Err(e) => {
                warn!(stage = "[ERROR]", attempt, error = %e, "Ultra order request network failure");
                last_error = Some(anyhow!("Ultra order network failure: {e}"));
                if attempt < MAX_QUOTE_ATTEMPTS {
                    tokio::time::sleep(Duration::from_millis(350)).await;
                }
                continue;
            }
        };

        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        info!(
            stage = "[QUOTE RESPONSE]",
            attempt,
            status = %status,
            body_len = body.len(),
            "Jupiter Ultra order HTTP response received"
        );

        if status == StatusCode::TOO_MANY_REQUESTS {
            return Err(anyhow!(
                "Ultra order rate limited HTTP 429; stopping cycle without retry spam: {}",
                &body[..body.len().min(500)]
            ));
        }

        if !status.is_success() {
            let err = anyhow!(
                "Ultra order HTTP {}: {}",
                status,
                &body[..body.len().min(500)]
            );
            warn!(stage = "[ERROR]", attempt, error = %err, "Ultra order HTTP failure");
            last_error = Some(err);
            if attempt < MAX_QUOTE_ATTEMPTS {
                tokio::time::sleep(Duration::from_millis(350)).await;
            }
            continue;
        }

        let raw: UltraOrderResponse = serde_json::from_str(&body)
            .map_err(|e| anyhow!("Ultra order parse failed: {e}; body={}", &body[..body.len().min(500)]))?;

        if let Some(code) = raw.error_code.as_deref() {
            return Err(anyhow!(
                "Ultra order API error {}: {}",
                code,
                raw.error_message.unwrap_or_else(|| "missing errorMessage".to_string())
            ));
        }

        let transaction_b64 = raw.transaction
            .filter(|v| !v.is_empty())
            .ok_or_else(|| anyhow!("Ultra order missing transaction field; taker may be invalid or wallet lacks required funds"))?;
        let request_id = raw.request_id
            .filter(|v| !v.is_empty())
            .ok_or_else(|| anyhow!("Ultra order missing requestId"))?;
        let out_amount = raw.out_amount.as_deref()
            .and_then(|s| s.parse::<u64>().ok())
            .ok_or_else(|| anyhow!("Ultra order missing or invalid outAmount"))?;
        let in_amount = raw.in_amount.as_deref()
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(amount);
        let price_impact_pct = raw.price_impact_pct.as_deref()
            .and_then(|s| s.parse::<f64>().ok())
            .or(raw.price_impact)
            .unwrap_or(0.0);

        let order = UltraOrder {
            input_mint: raw.input_mint.unwrap_or_else(|| input_mint.to_string()),
            output_mint: raw.output_mint.unwrap_or_else(|| output_mint.to_string()),
            in_amount,
            out_amount,
            price_impact_pct,
            route_plan: raw.route_plan.unwrap_or_default(),
            signature_fee_lamports: raw.signature_fee_lamports.unwrap_or(5_000),
            prioritization_fee_lamports: raw.prioritization_fee_lamports.unwrap_or(0),
            rent_fee_lamports: raw.rent_fee_lamports.unwrap_or(0),
            request_id,
            transaction_b64,
        };

        info!(
            stage = "[QUOTE RECEIVED]",
            input_mint = %order.input_mint,
            output_mint = %order.output_mint,
            in_amount = order.in_amount,
            out_amount = order.out_amount,
            price_impact_pct = order.price_impact_pct,
            route_count = order.route_plan.len(),
            request_id = %order.request_id,
            signature_fee_lamports = order.signature_fee_lamports,
            prioritization_fee_lamports = order.prioritization_fee_lamports,
            rent_fee_lamports = order.rent_fee_lamports,
            "Jupiter Ultra order received"
        );

        return Ok(order);
    }

    Err(last_error.unwrap_or_else(|| anyhow!("Ultra order failed after {MAX_QUOTE_ATTEMPTS} attempts")))
}
