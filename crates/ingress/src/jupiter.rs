// crates/ingress/src/jupiter.rs
// Jupiter Price Monitor - Fully Fixed (April 2026)

use common::types::{Dex, MarketEdge, TokenMint};
use reqwest::Client;
use rust_decimal::Decimal;
use serde::Deserialize;
use std::collections::HashMap;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::time::sleep;
use tracing::{info, warn};

const POLL_INTERVAL_MS: u64 = 10_000;
const HTTP_TIMEOUT_MS: u64 = 10_000;
const MAX_LOG_WEIGHT: f64 = 2.0;
const MIN_LOG_WEIGHT: f64 = -2.0;
const JUPITER_SLOT_SENTINEL: u64 = u64::MAX;

const BACKOFF_BASE_MS: u64 = 2_000;
const MAX_BACKOFF_MS: u64 = 30_000;

// ── Token universe ───────────────────────────────────────────────────────────
#[derive(Debug, Clone)]
pub struct KnownToken {
    pub symbol: &'static str,
    pub mint: &'static str,
    pub decimals: u32,
    pub quote_amount: u64,
    pub coingecko_id: &'static str,
}

pub use KnownToken as TokenInfo;

pub const TOKENS: &[KnownToken] = &[
    KnownToken { symbol: "SOL", mint: "So11111111111111111111111111111111111111112", decimals: 9, quote_amount: 1_000_000_000, coingecko_id: "solana" },
    KnownToken { symbol: "USDC", mint: "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v", decimals: 6, quote_amount: 1_000_000, coingecko_id: "usd-coin" },
    KnownToken { symbol: "USDT", mint: "Es9vMFrzaCERmJfrF4H2FYD4KCoNkY11McCe8BenwNYB", decimals: 6, quote_amount: 1_000_000, coingecko_id: "tether" },
    KnownToken { symbol: "ORCA", mint: "orcaEKTdK7LKz57vaAYr9QeNsVEPfiu6QeMU1kektZE", decimals: 6, quote_amount: 1_000_000, coingecko_id: "orca" },
    KnownToken { symbol: "JUP", mint: "JUPyiwrYJFskUPiHa7hkeR8VUtAeFoSYbKedZNsDvCN", decimals: 6, quote_amount: 1_000_000, coingecko_id: "jupiter-exchange-solana" },
    KnownToken { symbol: "mSOL", mint: "mSoLzYCxHdYgdzU16g5QSh3i5K3z3KZK7ytfqcJm7So", decimals: 9, quote_amount: 1_000_000_000, coingecko_id: "msol" },
    KnownToken { symbol: "JitoSOL", mint: "J1toso1uCk3RLmjorhTtrVwY9HJ7X8V9yYac6Y7kGCPn", decimals: 9, quote_amount: 1_000_000_000, coingecko_id: "jito-staked-sol" },
    KnownToken { symbol: "BONK", mint: "DezXAZ8z7PnrnRJjz3wXBoRgixCa6xjnB7YaB1pPB263", decimals: 5, quote_amount: 1_000_000, coingecko_id: "bonk" },
    KnownToken { symbol: "WIF", mint: "EKpQGSJtjMFqKZ9KQanSqYXRcF8fBopzLHYxdM65zcjm", decimals: 6, quote_amount: 1_000_000, coingecko_id: "dogwifcoin" },
    KnownToken { symbol: "RENDER", mint: "rndrizKT3MK1iimdxRdWabcF7Zg7AR5T4nud4EkHBof", decimals: 8, quote_amount: 1_000_000, coingecko_id: "render-token" },
];

pub fn build_token_info_map() -> HashMap<&'static str, &'static KnownToken> {
    TOKENS.iter().map(|t| (t.mint, t)).collect()
}

// ── Response types ───────────────────────────────────────────────────────────
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct JupiterPriceV3Item {
    usd_price: Option<f64>,
}

type CoinGeckoResponse = HashMap<String, HashMap<String, f64>>;

#[derive(Debug, Clone, PartialEq)]
enum PriceSource {
    JupiterV3,
    CoinGecko,
}

impl PriceSource {
    fn name(&self) -> &'static str {
        match self {
            PriceSource::JupiterV3 => "Jupiter Price v3 API",
            PriceSource::CoinGecko => "CoinGecko public API",
        }
    }
}

// ── JupiterMonitor ───────────────────────────────────────────────────────────
pub struct JupiterMonitor;

impl JupiterMonitor {
    #[must_use]
    pub fn spawn() -> mpsc::Receiver<Vec<MarketEdge>> {
        Self::spawn_with_key(None)
    }

    #[must_use]
    pub fn spawn_with_key(api_key: Option<String>) -> mpsc::Receiver<Vec<MarketEdge>> {
        let (tx, rx) = mpsc::channel(64);
        tokio::spawn(Self::run(tx, api_key));
        rx
    }

    async fn run(tx: mpsc::Sender<Vec<MarketEdge>>, api_key: Option<String>) {
        let client = Client::builder()
            .timeout(Duration::from_millis(HTTP_TIMEOUT_MS))
            .build()
            .expect("Failed to build HTTP client");

        let sol_mint = "So11111111111111111111111111111111111111112";
        info!("Price monitor started — poll every {}ms", POLL_INTERVAL_MS);

        let mut price_cache: HashMap<String, f64> = HashMap::new();
        let mut active_source = PriceSource::JupiterV3;
        let mut consecutive_failures: u32 = 0;
        let mut backoff_ms = BACKOFF_BASE_MS;

        loop {
            sleep(Duration::from_millis(POLL_INTERVAL_MS)).await;

            let result = match &active_source {
                PriceSource::JupiterV3 => Self::fetch_jupiter_v3(&client, api_key.as_deref()).await,
                PriceSource::CoinGecko => Self::fetch_coingecko(&client).await,
            };

            match result {
                Ok(prices) if !prices.is_empty() => {
                    if consecutive_failures > 0 {
                        info!("NEW API DISCOVERED: {}", active_source.name());
                    }
                    consecutive_failures = 0;
                    backoff_ms = BACKOFF_BASE_MS;

                    for (mint, price) in &prices {
                        price_cache.insert(mint.clone(), *price);
                    }

                    if let Some(&sol_price) = price_cache.get(sol_mint) {
                        if sol_price > 0.0 {
                            let edges = build_edges_from_prices(&price_cache, sol_price);
                            if !edges.is_empty() {
                                let _ = tx.send(edges).await;
                            }
                        }
                    }
                }
                _ => {
                    consecutive_failures += 1;
                    warn!(
                        source = active_source.name(),
                        failures = consecutive_failures,
                        "JUPITER API FAILED — SEARCHING FOR ALTERNATIVE"
                    );

                    active_source = Self::try_failover(&client, &active_source, &api_key, &mut price_cache, sol_mint, &tx).await;

                    if consecutive_failures > 3 {
                        sleep(Duration::from_millis(backoff_ms)).await;
                        backoff_ms = (backoff_ms * 2).min(MAX_BACKOFF_MS);
                    }
                }
            }
        }
    }

    async fn try_failover(
        client: &Client,
        current: &PriceSource,
        api_key: &Option<String>,
        price_cache: &mut HashMap<String, f64>,
        sol_mint: &str,
        tx: &mpsc::Sender<Vec<MarketEdge>>,
    ) -> PriceSource {
        let next = match current {
            PriceSource::JupiterV3 => PriceSource::CoinGecko,
            PriceSource::CoinGecko => PriceSource::JupiterV3,
        };

        let next_result = match &next {
            PriceSource::JupiterV3 => Self::fetch_jupiter_v3(client, api_key.as_deref()).await,
            PriceSource::CoinGecko => Self::fetch_coingecko(client).await,
        };

        match next_result {
            Ok(prices) if !prices.is_empty() => {
                info!("Switched to {}", next.name());
                for (mint, price) in &prices {
                    price_cache.insert(mint.clone(), *price);
                }
                next
            }
            _ => current.clone(),
        }
    }

    async fn fetch_jupiter_v3(
        client: &Client,
        api_key: Option<&str>,
    ) -> anyhow::Result<HashMap<String, f64>> {
        let ids = TOKENS.iter().map(|t| t.mint).collect::<Vec<_>>().join(",");
        let url = format!("https://api.jup.ag/price/v3?ids={}", ids);

        let mut req = client.get(&url).header("accept", "application/json");
        if let Some(key) = api_key {
            req = req.header("x-api-key", key);
        }

        let resp = req.send().await?;
        let status = resp.status();

        if !status.is_success() {
            let text = resp.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!("Jupiter v3 HTTP {}: {}", status, &text[..text.len().min(300)]));
        }

        let body = resp.text().await?;
        let data: HashMap<String, JupiterPriceV3Item> = serde_json::from_str(&body)
            .map_err(|e| anyhow::anyhow!("JSON parse error: {}", e))?;

        let mut prices = HashMap::new();
        for (mint, item) in data {
            if let Some(p) = item.usd_price {
                if p > 0.0 {
                    prices.insert(mint, p);
                }
            }
        }
        Ok(prices)
    }

    async fn fetch_coingecko(client: &Client) -> anyhow::Result<HashMap<String, f64>> {
        let coin_ids: Vec<&str> = TOKENS
            .iter()
            .filter(|t| !t.coingecko_id.is_empty())
            .map(|t| t.coingecko_id)
            .collect();

        if coin_ids.is_empty() {
            return Ok(HashMap::new());
        }

        let url = format!(
            "https://api.coingecko.com/api/v3/simple/price?ids={}&vs_currencies=usd",
            coin_ids.join(",")
        );

        let resp = client.get(&url).header("Accept", "application/json").send().await?;
        let status = resp.status();

        if !status.is_success() {
            let text = resp.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!("CoinGecko HTTP {}: {}", status, &text[..text.len().min(200)]));
        }

        let cg: CoinGeckoResponse = resp.json().await?;
        let id_to_mint: HashMap<&str, &str> = TOKENS
            .iter()
            .filter(|t| !t.coingecko_id.is_empty())
            .map(|t| (t.coingecko_id, t.mint))
            .collect();

        let mut prices = HashMap::new();
        for (cg_id, map) in &cg {
            if let Some(&usd) = map.get("usd") {
                if usd > 0.0 {
                    if let Some(&mint) = id_to_mint.get(cg_id.as_str()) {
                        prices.insert(mint.to_string(), usd);
                    }
                }
            }
        }
        Ok(prices)
    }
}

// ── Helpers ──────────────────────────────────────────────────────────────────
fn mint_to_token_mint(mint: &str) -> TokenMint {
    let mut arr = [0u8; 32];
    if let Ok(b) = bs58::decode(mint).into_vec() {
        let len = b.len().min(32);
        arr[..len].copy_from_slice(&b[..len]);
    }
    TokenMint::new(arr)
}

fn build_edges_from_prices(
    price_cache: &HashMap<String, f64>,
    sol_price_usd: f64,
) -> Vec<MarketEdge> {
    let mut edges = Vec::new();
    let liq_estimate = ((sol_price_usd * 1_000_000.0) as u64).max(1_000_000_000);

    for i in 0..TOKENS.len() {
        let ta = &TOKENS[i];
        let price_a = match price_cache.get(ta.mint) {
            Some(&p) if p > 0.0 => p,
            _ => continue,
        };

        for j in 0..TOKENS.len() {
            if i == j { continue; }
            let tb = &TOKENS[j];
            let price_b = match price_cache.get(tb.mint) {
                Some(&p) if p > 0.0 => p,
                _ => continue,
            };

            let raw = -(price_b / price_a).ln();
            let clamped = raw.clamp(MIN_LOG_WEIGHT, MAX_LOG_WEIGHT);
            let log_weight = Decimal::try_from(clamped).unwrap_or(Decimal::ZERO);

            let dexes = if i < 4 && j < 4 {
                vec![Dex::JupiterV6, Dex::Raydium, Dex::Orca, Dex::Meteora, Dex::Phoenix]
            } else {
                vec![Dex::JupiterV6, Dex::Raydium]
            };

            for dex in dexes {
                edges.push(MarketEdge {
                    from: mint_to_token_mint(ta.mint),
                    to: mint_to_token_mint(tb.mint),
                    dex,
                    log_weight,
                    liquidity_lamports: liq_estimate,
                    slot: JUPITER_SLOT_SENTINEL,
                });
            }
        }
    }
    edges
}
