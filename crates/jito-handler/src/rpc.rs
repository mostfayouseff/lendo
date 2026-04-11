// =============================================================================
// SECURITY AUDIT CHECKLIST — jito-handler/src/rpc.rs
// [✓] RPC URL loaded from config — never hardcoded
// [✓] HTTP timeout enforced — never blocks indefinitely
// [✓] Response errors propagated — no silent failures
// [✓] No private key material transmitted through RPC
// [✓] No unsafe code
//
// SOLANA RPC CLIENT
//
// Provides the minimal Solana RPC calls needed for live trading:
//   1. getLatestBlockhash — required to build valid transactions
//   2. getBalance         — to verify operator account has sufficient funds
//   3. simulateTransaction — optional local verification before Jito submission
//
// Uses the standard JSON-RPC 2.0 protocol over HTTPS.
// =============================================================================

use anyhow::{Context, Result};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tracing::{debug, warn};

const RPC_TIMEOUT_MS: u64 = 5_000;
const MAX_RETRIES: u32 = 3;
const RETRY_DELAY_MS: u64 = 500;

/// A recent blockhash with its associated last-valid block height.
#[derive(Debug, Clone)]
pub struct RecentBlockhash {
    /// Base58-encoded blockhash string
    pub blockhash: String,
    /// Block height after which this blockhash expires
    pub last_valid_block_height: u64,
}

// ── JSON-RPC types ────────────────────────────────────────────────────────────

#[derive(Serialize)]
struct RpcRequest<'a, T: Serialize> {
    jsonrpc: &'static str,
    id: u64,
    method: &'a str,
    params: T,
}

#[derive(Deserialize, Debug)]
struct RpcResponse<T> {
    result: Option<T>,
    error: Option<RpcError>,
}

#[derive(Deserialize, Debug)]
struct RpcError {
    code: i64,
    message: String,
}

#[derive(Deserialize, Debug)]
struct BlockhashResult {
    value: BlockhashValue,
}

#[derive(Deserialize, Debug)]
struct BlockhashValue {
    blockhash: String,
    #[serde(rename = "lastValidBlockHeight")]
    last_valid_block_height: u64,
}

#[derive(Deserialize, Debug)]
struct SlotResult(u64);

#[derive(Deserialize, Debug)]
struct BalanceResult {
    value: u64,
}

#[derive(Deserialize, Debug)]
struct SimulateResult {
    value: SimulateValue,
}

#[derive(Deserialize, Debug)]
struct SimulateValue {
    err: Option<serde_json::Value>,
    logs: Option<Vec<String>>,
}

#[derive(Deserialize, Debug)]
pub struct SignatureStatus {
    pub slot: u64,
    pub confirmations: Option<u64>,
    pub err: Option<serde_json::Value>,
    #[serde(rename = "confirmationStatus")]
    pub confirmation_status: Option<String>,
}

#[derive(Deserialize, Debug)]
struct SignatureStatusesResult {
    value: Vec<Option<SignatureStatus>>,
}

// ── Client ────────────────────────────────────────────────────────────────────

/// Minimal Solana JSON-RPC client.
pub struct SolanaRpcClient {
    client: Client,
    url: String,
}

impl SolanaRpcClient {
    /// Construct a new RPC client.
    ///
    /// # Errors
    /// Returns error if the HTTP client cannot be built.
    pub fn new(rpc_url: &str) -> Result<Self> {
        let client = Client::builder()
            .timeout(Duration::from_millis(RPC_TIMEOUT_MS))
            .build()
            .context("Failed to build RPC HTTP client")?;

        Ok(Self {
            client,
            url: rpc_url.to_string(),
        })
    }

    /// Fetch the current slot — used as a lightweight health-check.
    ///
    /// # Errors
    /// Returns error if the RPC call fails.
    pub async fn get_slot(&self) -> Result<u64> {
        let request = RpcRequest {
            jsonrpc: "2.0",
            id: 4,
            method: "getSlot",
            params: serde_json::json!([{"commitment": "confirmed"}]),
        };
        let result = self.post::<SlotResult>(&request).await?;
        debug!(slot = result.0, "getSlot OK");
        Ok(result.0)
    }

    /// Fetch the latest blockhash (required for all transactions).
    ///
    /// Retries up to `MAX_RETRIES` times on transient failures.
    ///
    /// # Errors
    /// Returns error if the RPC call fails after all retries.
    pub async fn get_latest_blockhash(&self) -> Result<RecentBlockhash> {
        let request = RpcRequest {
            jsonrpc: "2.0",
            id: 1,
            method: "getLatestBlockhash",
            params: serde_json::json!([{"commitment": "confirmed"}]),
        };

        for attempt in 1..=MAX_RETRIES {
            match self.post::<BlockhashResult>(&request).await {
                Ok(result) => {
                    let bh = RecentBlockhash {
                        blockhash: result.value.blockhash,
                        last_valid_block_height: result.value.last_valid_block_height,
                    };
                    debug!(
                        blockhash = %bh.blockhash,
                        last_valid = bh.last_valid_block_height,
                        "Latest blockhash fetched"
                    );
                    return Ok(bh);
                }
                Err(e) if attempt < MAX_RETRIES => {
                    warn!(attempt, "RPC getLatestBlockhash failed: {e} — retrying");
                    tokio::time::sleep(Duration::from_millis(RETRY_DELAY_MS)).await;
                }
                Err(e) => return Err(e),
            }
        }
        Err(anyhow::anyhow!("getLatestBlockhash failed after {MAX_RETRIES} retries"))
    }

    /// Get the lamport balance for a base58-encoded account address.
    ///
    /// # Errors
    /// Returns error if the RPC call fails.
    pub async fn get_balance(&self, pubkey_b58: &str) -> Result<u64> {
        let request = RpcRequest {
            jsonrpc: "2.0",
            id: 2,
            method: "getBalance",
            params: serde_json::json!([pubkey_b58, {"commitment": "confirmed"}]),
        };

        let result = self.post::<BalanceResult>(&request).await?;
        debug!(
            pubkey = %pubkey_b58,
            balance_lamports = result.value,
            "Balance fetched"
        );
        Ok(result.value)
    }

    /// Simulate a base64-encoded transaction and return whether it would succeed.
    ///
    /// # Errors
    /// Returns error if the RPC call fails at the network level.
    pub async fn simulate_transaction(&self, tx_b64: &str) -> Result<bool> {
        let request = RpcRequest {
            jsonrpc: "2.0",
            id: 3,
            method: "simulateTransaction",
            params: serde_json::json!([
                tx_b64,
                {"commitment": "confirmed", "encoding": "base64"}
            ]),
        };

        let result = self.post::<SimulateResult>(&request).await?;
        let success = result.value.err.is_none();

        if let Some(err) = &result.value.err {
            warn!(error = ?err, "Simulation rejected transaction");
        }

        if let Some(logs) = &result.value.logs {
            for log in logs.iter().take(10) {
                if success {
                    debug!(log = %log, "Simulation log");
                } else {
                    warn!(log = %log, "Simulation rejection log");
                }
            }
        }

        Ok(success)
    }

    pub async fn send_transaction_base64(&self, tx_b64: &str) -> Result<String> {
        let request = RpcRequest {
            jsonrpc: "2.0",
            id: 5,
            method: "sendTransaction",
            params: serde_json::json!([
                tx_b64,
                {
                    "encoding": "base64",
                    "skipPreflight": false,
                    "preflightCommitment": "confirmed",
                    "maxRetries": 3
                }
            ]),
        };

        self.post::<String>(&request).await
    }

    pub async fn get_signature_status(&self, signature: &str) -> Result<Option<SignatureStatus>> {
        let request = RpcRequest {
            jsonrpc: "2.0",
            id: 6,
            method: "getSignatureStatuses",
            params: serde_json::json!([
                [signature],
                {"searchTransactionHistory": true}
            ]),
        };

        let result = self.post::<SignatureStatusesResult>(&request).await?;
        Ok(result.value.into_iter().next().flatten())
    }

    pub async fn confirm_transaction(&self, signature: &str, attempts: u32, delay_ms: u64) -> Result<SignatureStatus> {
        for attempt in 1..=attempts {
            match self.get_signature_status(signature).await? {
                Some(status) if status.err.is_none() => {
                    let confirmed = status.confirmation_status.as_deref() == Some("confirmed")
                        || status.confirmation_status.as_deref() == Some("finalized")
                        || status.confirmations.is_none();
                    if confirmed {
                        return Ok(status);
                    }
                }
                Some(status) => {
                    return Err(anyhow::anyhow!(
                        "transaction failed at slot {}: {:?}",
                        status.slot,
                        status.err
                    ));
                }
                None => {}
            }

            if attempt < attempts {
                tokio::time::sleep(Duration::from_millis(delay_ms)).await;
            }
        }

        Err(anyhow::anyhow!(
            "transaction confirmation timed out after {attempts} attempts"
        ))
    }

    async fn post<T: for<'de> Deserialize<'de>>(
        &self,
        request: &(impl Serialize + ?Sized),
    ) -> Result<T> {
        let resp = self
            .client
            .post(&self.url)
            .json(request)
            .send()
            .await
            .context("RPC HTTP request failed")?;

        let status = resp.status();
        if !status.is_success() {
            return Err(anyhow::anyhow!("RPC HTTP error: {status}"));
        }

        let rpc_resp: RpcResponse<T> = resp.json().await.context("RPC response parse failed")?;

        if let Some(err) = rpc_resp.error {
            return Err(anyhow::anyhow!(
                "RPC error {}: {}",
                err.code,
                err.message
            ));
        }

        rpc_resp
            .result
            .ok_or_else(|| anyhow::anyhow!("RPC response missing 'result' field"))
    }
}
