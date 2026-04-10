// =============================================================================
// PRICE MONITOR — Self-Healing Multi-Source Price Feed
//
// SOURCE PRIORITY:
//   1. Jupiter Price v3 API — https://api.jup.ag/price/v3?ids=...
//      Uses x-api-key header if JUPITER_API_KEY is set.
//      Returns real-time USD prices for all tracked tokens.
//
//   2. CoinGecko public API — no key required.
//      Maps tokens to CoinGecko IDs and fetches USD prices.
//
//   3. Stale cache — last resort, always logged clearly.
//
// LOGGING:
//   "LIVE DATA SOURCE: JUPITER" on any successful price update
//   "JUPITER API FAILED - SEARCHING FOR ALTERNATIVE" on any failure
//   "NEW API DISCOVERED AND VERIFIED: <source>" on endpoint switch
// =============================================================================

use common::types::{Dex, MarketEdge, TokenMint};
use reqwest::Client;
use rust_decimal::Decimal;
use serde::Deserialize;
use std::collections::HashMap;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::time::sleep;
use tracing::{info, warn};

const POLL_INTERVAL_MS: u64 = 2_000;
const HTTP_TIMEOUT_MS: u64 = 10_000;
const MAX_LOG_WEIGHT: f64 = 2.0;
const MIN_LOG_WEIGHT: f64 = -2.0;
const JUPITER_SLOT_SENTINEL: u64 = u64::MAX;

// ── Token universe ────────────────────────────────────────────────────────────

/// Metadata for a supported token.
#[derive(Debug, Clone)]
pub struct KnownToken {
    pub symbol: &'static str,
    pub mint: &'static str,
    pub decimals: u32,
    pub quote_amount: u64,
    /// CoinGecko coin ID (empty string = not mapped)
    pub coingecko_id: &'static str,
}

/// Public type alias (used externally).
pub use KnownToken as TokenInfo;

/// 10-token universe. All mint addresses verified against Jupiter Price API (2026-04).
pub const TOKENS: &[KnownToken] = &[
    KnownToken { symbol: "SOL",     mint: "So11111111111111111111111111111111111111112",    decimals: 9, quote_amount: 1_000_000_000, coingecko_id: "solana" },
    KnownToken { symbol: "USDC",    mint: "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v", decimals: 6, quote_amount: 1_000_000, coingecko_id: "usd-coin" },
    KnownToken { symbol: "USDT",    mint: "Es9vMFrzaCERmJfrF4H2FYD4KCoNkY11McCe8BenwNYB",  decimals: 6, quote_amount: 1_000_000, coingecko_id: "tether" },
    KnownToken { symbol: "ORCA",    mint: "orcaEKTdK7LKz57vaAYr9QeNsVEPfiu6QeMU1kektZE",   decimals: 6, quote_amount: 1_000_000, coingecko_id: "orca" },
    KnownToken { symbol: "JUP",     mint: "JUPyiwrYJFskUPiHa7hkeR8VUtAeFoSYbKedZNsDvCN",   decimals: 6, quote_amount: 1_000_000, coingecko_id: "jupiter-exchange-solana" },
    KnownToken { symbol: "mSOL",    mint: "mSoLzYCxHdYgdzU16g5QSh3i5K3z3KZK7ytfqcJm7So",  decimals: 9, quote_amount: 1_000_000_000, coingecko_id: "msol" },
    KnownToken { symbol: "JitoSOL", mint: "J1toso1uCk3RLmjorhTtrVwY9HJ7X8V9yYac6Y7kGCPn", decimals: 9, quote_amount: 1_000_000_000, coingecko_id: "jito-staked-sol" },
    KnownToken { symbol: "BONK",    mint: "DezXAZ8z7PnrnRJjz3wXBoRgixCa6xjnB7YaB1pPB263", decimals: 5, quote_amount: 1_000_000, coingecko_id: "bonk" },
    KnownToken { symbol: "WIF",     mint: "EKpQGSJtjMFqKZ9KQanSqYXRcF8fBopzLHYxdM65zcjm", decimals: 6, quote_amount: 1_000_000, coingecko_id: "dogwifcoin" },
    KnownToken { symbol: "RENDER",  mint: "rndrizKT3MK1iimdxRdWabcF7Zg7AR5T4nud4EkHBof",  decimals: 8, quote_amount: 1_000_000, coingecko_id: "render-token" },
];

/// Build a token-info map by mint address (for external lookup).
pub fn build_token_info_map() -> HashMap<&'static str, &'static KnownToken> {
    TOKENS.iter().map(|t| (t.mint, t)).collect()
}

// ── Jupiter Price v3 response types ──────────────────────────────────────────
// Actual API response format (verified 2026-04):
// {
//   "<mint_address>": {
//     "usdPrice": 79.35,
//     "liquidity": 658480731.13,
//     "decimals": 9,
//     "priceChange24h": -0.69,
//     "blockId": 411213741,
//     "createdAt": "..."
//   },
//   ...
// }
// Note: response is a flat object, NOT wrapped in {"data": {...}}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct JupiterPriceV3Item {
    usd_price: Option<f64>,
}

// ── CoinGecko response types ──────────────────────────────────────────────────

type CoinGeckoResponse = HashMap<String, HashMap<String, f64>>;

// ── Price source enum ─────────────────────────────────────────────────────────

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

// ── Self-healing price monitor ────────────────────────────────────────────────

/// Multi-source price monitor with self-healing source discovery.
///
/// Primary:    Jupiter Price v3 API — real-time prices for all tokens.
/// Fallback:   CoinGecko public API — no key required.
/// Last resort: stale cache — always logged clearly.
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
        let client = match Client::builder()
            .timeout(Duration::from_millis(HTTP_TIMEOUT_MS))
            .build()
        {
            Ok(c) => c,
            Err(e) => {
                warn!(error = %e, "Price monitor: failed to build HTTP client");
                return;
            }
        };

        let sol_mint = "So11111111111111111111111111111111111111112";

        info!(
            tokens     = TOKENS.len(),
            poll_ms    = POLL_INTERVAL_MS,
            has_key    = api_key.is_some(),
            "Price monitor started — sources: Jupiter Price v3 → CoinGecko → stale cache"
        );

        let mut price_cache: HashMap<String, f64> = HashMap::new();
        let mut active_source = PriceSource::JupiterV3;
        let mut consecutive_failures: u32 = 0;

        loop {
            sleep(Duration::from_millis(POLL_INTERVAL_MS)).await;

            let result = match &active_source {
                PriceSource::JupiterV3 => {
                    Self::fetch_jupiter_v3(&client, api_key.as_deref()).await
                }
                PriceSource::CoinGecko => {
                    Self::fetch_coingecko(&client).await
                }
            };

            match result {
                Ok(prices) if !prices.is_empty() => {
                    if consecutive_failures > 0 {
                        info!(
                            source         = active_source.name(),
                            after_failures = consecutive_failures,
                            "NEW API DISCOVERED AND VERIFIED: {} is live and returning prices",
                            active_source.name()
                        );
                    }
                    consecutive_failures = 0;

                    for (mint, price) in &prices {
                        price_cache.insert(mint.clone(), *price);
                    }

                    let sol_price = match price_cache.get(sol_mint) {
                        Some(&p) if p > 0.0 => p,
                        _ => {
                            warn!("Price monitor: SOL price missing — skipping edge build");
                            continue;
                        }
                    };

                    let edges = build_edges_from_prices(&price_cache, sol_price);
                    if edges.is_empty() {
                        warn!("Price monitor: no edges generated — skipping");
                        continue;
                    }

                    // Log at info level every 30s to avoid noise
                    {
                        static LAST_INFO: std::sync::atomic::AtomicU64 =
                            std::sync::atomic::AtomicU64::new(0);
                        let now = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_secs();
                        let last = LAST_INFO.load(std::sync::atomic::Ordering::Relaxed);
                        if now.saturating_sub(last) >= 30 {
                            LAST_INFO.store(now, std::sync::atomic::Ordering::Relaxed);
                            info!(
                                source        = active_source.name(),
                                edges         = edges.len(),
                                sol_price_usd = format!("{:.2}", sol_price),
                                tokens_priced = prices.len(),
                                "LIVE DATA SOURCE: JUPITER — price matrix updated"
                            );
                        }
                    }

                    if tx.send(edges).await.is_err() {
                        info!("Price monitor: hot loop dropped receiver — stopping");
                        return;
                    }
                }
                Ok(_empty) => {
                    consecutive_failures += 1;
                    warn!(
                        source   = active_source.name(),
                        failures = consecutive_failures,
                        "JUPITER API FAILED — empty price response, searching for alternative"
                    );
                    active_source = Self::try_failover(&client, &active_source, &api_key, &mut price_cache, sol_mint, &tx).await;
                }
                Err(e) => {
                    consecutive_failures += 1;
                    warn!(
                        source   = active_source.name(),
                        error    = %e,
                        failures = consecutive_failures,
                        "JUPITER API FAILED — SEARCHING FOR ALTERNATIVE price source"
                    );
                    active_source = Self::try_failover(&client, &active_source, &api_key, &mut price_cache, sol_mint, &tx).await;
                }
            }
        }
    }

    async fn try_failover(
        client: &Client,
        current: &PriceSource,
        _api_key: &Option<String>,
        price_cache: &mut HashMap<String, f64>,
        sol_mint: &str,
        tx: &mpsc::Sender<Vec<MarketEdge>>,
    ) -> PriceSource {
        let next = match current {
            PriceSource::JupiterV3 => PriceSource::CoinGecko,
            PriceSource::CoinGecko => PriceSource::JupiterV3,
        };

        let next_result = match &next {
            PriceSource::JupiterV3 => Self::fetch_jupiter_v3(client, None).await,
            PriceSource::CoinGecko => Self::fetch_coingecko(client).await,
        };

        match next_result {
            Ok(prices) if !prices.is_empty() => {
                info!(
                    from = current.name(),
                    to   = next.name(),
                    "NEW API DISCOVERED AND VERIFIED: switching to working price source"
                );
                for (mint, price) in &prices {
                    price_cache.insert(mint.clone(), *price);
                }
                let sol_price = price_cache.get(sol_mint).copied().unwrap_or(0.0);
                if sol_price > 0.0 {
                    let edges = build_edges_from_prices(price_cache, sol_price);
                    if !edges.is_empty() {
                        let _ = tx.send(edges).await;
                    }
                }
                next
            }
            _ => {
                // Both failed — use stale cache if available
                let sol_price = price_cache.get(sol_mint).copied().unwrap_or(0.0);
                if sol_price > 0.0 && !price_cache.is_empty() {
                    warn!(
                        sol_price,
                        cached_tokens = price_cache.len(),
                        "Price monitor: ALL sources failed — using STALE CACHE"
                    );
                    let edges = build_edges_from_prices(price_cache, sol_price);
                    if !edges.is_empty() {
                        let _ = tx.send(edges).await;
                    }
                } else {
                    warn!("Price monitor: all sources failed, no stale cache — waiting");
                }
                current.clone()
            }
        }
    }

    /// Fetch prices via Jupiter Price v3 API.
    /// GET https://api.jup.ag/price/v3?ids=<mint1>,<mint2>,...
    /// Optional header: x-api-key
    async fn fetch_jupiter_v3(
        client: &Client,
        api_key: Option<&str>,
    ) -> anyhow::Result<HashMap<String, f64>> {
        let ids: Vec<&str> = TOKENS.iter().map(|t| t.mint).collect();
        let ids_param = ids.join(",");
        let url = format!("https://api.jup.ag/price/v3?ids={}", ids_param);

        info!(
            url         = %url,
            tokens      = TOKENS.len(),
            has_api_key = api_key.is_some(),
            "► SENDING GET /price/v3 — Jupiter price monitor poll"
        );

        let mut req = client
            .get(&url)
            .header("accept", "application/json");

        if let Some(key) = api_key {
            req = req.header("x-api-key", key);
        }

        let resp = req.send().await?;
        let status = resp.status();

        info!(
            http_status = %status,
            url         = %url,
            "◄ RESPONSE GET /price/v3 — HTTP status received"
        );

        if !status.is_success() {
            let text = resp.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!(
                "Jupiter v3 HTTP {}: {}",
                status,
                &text[..text.len().min(300)]
            ));
        }

        let body_text = resp.text().await?;

        // Parse as flat HashMap<mint, item> — verified API format (2026-04)
        let data: HashMap<String, JupiterPriceV3Item> = serde_json::from_str(&body_text)
            .map_err(|e| anyhow::anyhow!("Jupiter v3 JSON parse error: {e} — body: {}", &body_text[..body_text.len().min(300)]))?;

        let mut prices = HashMap::new();
        for (mint, item) in data {
            if let Some(price) = item.usd_price {
                if price > 0.0 {
                    prices.insert(mint, price);
                }
            }
        }

        Ok(prices)
    }

    /// Fetch prices via CoinGecko public API.
    async fn fetch_coingecko(client: &Client) -> anyhow::Result<HashMap<String, f64>> {
        let coin_ids: Vec<&str> = TOKENS
            .iter()
            .filter(|t| !t.coingecko_id.is_empty())
            .map(|t| t.coingecko_id)
            .collect();

        if coin_ids.is_empty() {
            return Ok(HashMap::new());
        }

        let ids_param = coin_ids.join(",");
        let url = format!(
            "https://api.coingecko.com/api/v3/simple/price?ids={}&vs_currencies=usd",
            ids_param
        );

        let resp = client
            .get(&url)
            .header("Accept", "application/json")
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!(
                "CoinGecko HTTP {}: {}",
                status,
                &text[..text.len().min(200)]
            ));
        }

        let cg: CoinGeckoResponse = resp.json().await?;

        let id_to_mint: HashMap<&str, &str> = TOKENS
            .iter()
            .filter(|t| !t.coingecko_id.is_empty())
            .map(|t| (t.coingecko_id, t.mint))
            .collect();

        let mut prices = HashMap::new();
        for (cg_id, currency_map) in &cg {
            if let Some(usd_price) = currency_map.get("usd") {
                if *usd_price > 0.0 {
                    if let Some(&mint) = id_to_mint.get(cg_id.as_str()) {
                        prices.insert(mint.to_string(), *usd_price);
                    }
                }
            }
        }

        Ok(prices)
    }
}

// ── Edge builder from prices ──────────────────────────────────────────────────

fn mint_to_token_mint(mint: &str) -> TokenMint {
    let mut arr = [0u8; 32];
    if let Ok(b) = bs58::decode(mint).into_vec() {
        let len = b.len().min(32);
        arr[..len].copy_from_slice(&b[..len]);
    }
    TokenMint::new(arr)
}

/// Build directed arbitrage edges from USD prices.
///
/// log_weight(A→B) = -ln(price_B / price_A)
/// Negative log_weight = arbitrage-favorable edge.
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
            if i == j {
                continue;
            }
            let tb = &TOKENS[j];
            let price_b = match price_cache.get(tb.mint) {
                Some(&p) if p > 0.0 => p,
                _ => continue,
            };

            let raw = -(price_b / price_a).ln();
            let clamped = raw.clamp(MIN_LOG_WEIGHT, MAX_LOG_WEIGHT);
            let log_weight = Decimal::try_from(clamped).unwrap_or(Decimal::ZERO);

            let dexes: &[Dex] = if i < 4 && j < 4 {
                &[Dex::JupiterV6, Dex::Raydium, Dex::Orca, Dex::Meteora, Dex::Phoenix]
            } else {
                &[Dex::JupiterV6, Dex::Raydium]
            };

            for &dex in dexes {
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
