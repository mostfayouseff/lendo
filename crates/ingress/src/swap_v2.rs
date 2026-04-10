// =============================================================================
// JUPITER SWAP V2 — BUILD INSTRUCTIONS
//
// Endpoint: GET https://api.jup.ag/swap/v2/build
// Base URL:  https://api.jup.ag/swap/v2
// Auth:      x-api-key header (mandatory in production)
//
// Used ONLY for building atomic transactions:
//   • Returns swap instructions with full account metas
//   • Returns Address Lookup Table (ALT) addresses
//   • ALT account data is fetched from Solana RPC to resolve pubkeys
//   • Metis routing — optimised for on-chain atomic execution
//
// Instruction usage in atomic transaction:
//   [ComputeBudget]   ← from build response
//   [FlashLoanBorrow] ← from jito_handler::flash_tx_v2
//   [Setup + Swap]    ← from build response (swap1 leg)
//   [Setup + Swap]    ← from build response (swap2 leg, if triangular arb)
//   [FlashLoanRepay]  ← from jito_handler::flash_tx_v2
//   [JitoTip]         ← SystemProgram::Transfer to tip account
//
// FORBIDDEN endpoints (must not appear in this system):
//   /swap/v1/*  |  ultra/v1/*  |  /swap-instructions  |  lite-api.jup.ag
//   POST /execute
// =============================================================================

use anyhow::{anyhow, Context, Result};
use base64::Engine as _;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Duration;
use tracing::{debug, info, warn};

use crate::detection::SWAP_V2_BASE;

const BUILD_TIMEOUT_MS: u64 = 10_000;
const SYSTEM_PROGRAM: &str = "11111111111111111111111111111111";

// ── Shared instruction types ────────────────────────────────────────────────────

/// A single account reference in a Jupiter instruction.
#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct V2AccountMeta {
    pub pubkey:     String,
    pub is_signer:  bool,
    pub is_writable: bool,
}

/// A single on-chain instruction as returned by Jupiter /build.
#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct V2Instruction {
    pub program_id: String,
    pub accounts:   Vec<V2AccountMeta>,
    /// Base64-encoded raw instruction data.
    pub data:       String,
}

impl V2Instruction {
    /// Decode instruction data bytes from base64.
    pub fn data_bytes(&self) -> Result<Vec<u8>> {
        base64::engine::general_purpose::STANDARD
            .decode(&self.data)
            .context("V2Instruction: base64 decode failed")
    }
}

// ── /build response types ──────────────────────────────────────────────────────

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct BuildResponse {
    pub compute_budget_instructions:          Option<Vec<V2Instruction>>,
    pub setup_instructions:                   Option<Vec<V2Instruction>>,
    pub swap_instruction:                     Option<V2Instruction>,
    pub cleanup_instruction:                  Option<V2Instruction>,
    pub other_instructions:                   Option<Vec<V2Instruction>>,
    /// List of Address Lookup Table addresses (base58).
    /// Full table contents are fetched via Solana RPC.
    pub address_lookup_table_addresses:       Option<Vec<String>>,
    /// Alternative field name used by some API versions.
    pub addresses_by_lookup_table_address:    Option<HashMap<String, Vec<String>>>,
    pub blockhash:                            Option<String>,
    pub last_valid_block_height:              Option<u64>,
    pub prioritization_fee_lamports:          Option<u64>,
}

// ── Processed result ──────────────────────────────────────────────────────────

/// Fully resolved swap instructions ready for atomic transaction assembly.
///
/// The `alt_map` field maps each ALT address to its ordered list of account
/// pubkeys as fetched from the Solana RPC.  This is required to build the
/// `MessageAddressTableLookup` entries in a v0 VersionedTransaction.
#[derive(Debug, Clone)]
pub struct BuildInstructions {
    /// ComputeBudget instructions (SetComputeUnitLimit / SetComputeUnitPrice).
    pub compute_budget_instructions: Vec<V2Instruction>,
    /// ATA creation or wSOL wrap instructions.
    pub setup_instructions:          Vec<V2Instruction>,
    /// The core swap instruction.
    pub swap_instruction:            V2Instruction,
    /// Cleanup (ATA close / wSOL unwrap).
    pub cleanup_instruction:         Option<V2Instruction>,
    /// Additional instructions returned by the API.
    pub other_instructions:          Vec<V2Instruction>,
    /// ALT address → ordered list of all pubkeys in that table (from RPC).
    pub alt_map:                     HashMap<String, Vec<String>>,
    /// Recent blockhash (base58).
    pub blockhash:                   String,
    /// Last valid block height for this blockhash.
    pub last_valid_block_height:     u64,
    /// Suggested priority fee from Jupiter.
    pub prioritization_fee_lamports: u64,
}

impl BuildInstructions {
    /// All instructions in canonical order for atomic transaction assembly.
    ///
    /// Order: [ComputeBudget...] [Setup...] [Swap] [Cleanup?] [Other...]
    ///
    /// Note: FlashLoanBorrow and FlashLoanRepay are injected by the caller
    /// around these instructions.
    pub fn ordered_swap_instructions(&self) -> Vec<V2Instruction> {
        let mut out = Vec::new();
        out.extend(self.compute_budget_instructions.iter().cloned());
        out.extend(self.setup_instructions.iter().cloned());
        out.push(self.swap_instruction.clone());
        if let Some(ref c) = self.cleanup_instruction {
            out.push(c.clone());
        }
        out.extend(self.other_instructions.iter().cloned());
        out
    }

    /// Swap-only instructions (setup + swap + cleanup), without ComputeBudget.
    /// Used when combining two swap legs in one transaction (the caller handles
    /// a single shared ComputeBudget at the top of the tx).
    pub fn swap_only_instructions(&self) -> Vec<V2Instruction> {
        let mut out = Vec::new();
        out.extend(self.setup_instructions.iter().cloned());
        out.push(self.swap_instruction.clone());
        if let Some(ref c) = self.cleanup_instruction {
            out.push(c.clone());
        }
        out
    }
}

// ── GET /build ─────────────────────────────────────────────────────────────────

/// Fetch atomic swap instructions from Jupiter Swap V2 GET /build.
///
/// This is the ONLY correct way to build MEV transactions:
///   1. Call this function with the swap parameters.
///   2. Inject the returned instructions into an atomic transaction alongside
///      FlashLoanBorrow and FlashLoanRepay instructions.
///   3. Sign and simulate before submitting.
///
/// ALT account data is automatically resolved via `rpc_url` (Solana getAccountInfo).
///
/// # Errors
/// Returns an error if /build fails — caller MUST skip this loop iteration.
pub async fn get_build_instructions(
    client:       &Client,
    rpc_url:      &str,
    input_mint:   &str,
    output_mint:  &str,
    amount:       u64,
    user_pubkey:  &str,
    slippage_bps: u16,
    api_key:      Option<&str>,
) -> Result<BuildInstructions> {
    if input_mint == output_mint {
        return Err(anyhow!("get_build_instructions: input_mint == output_mint"));
    }
    if amount == 0 {
        return Err(anyhow!("get_build_instructions: amount must be > 0"));
    }

    let url = format!(
        "{}/build?inputMint={}&outputMint={}&amount={}&userPublicKey={}\
         &slippageBps={}&wrapAndUnwrapSol=true&useSharedAccounts=true\
         &dynamicComputeUnitLimit=true&onlyDirectRoutes=false",
        SWAP_V2_BASE, input_mint, output_mint, amount, user_pubkey, slippage_bps
    );

    info!(
        url          = %url,
        input_mint,
        output_mint,
        amount,
        user_pubkey,
        slippage_bps,
        has_api_key  = api_key.is_some(),
        "► SENDING GET /swap/v2/build — atomic instruction request"
    );

    let mut req = client
        .get(&url)
        .header("accept", "application/json")
        .timeout(Duration::from_millis(BUILD_TIMEOUT_MS));

    if let Some(key) = api_key {
        req = req.header("x-api-key", key);
    }

    let resp = req.send().await?;
    let status = resp.status();

    info!(
        status = %status,
        url    = %url,
        "◄ RESPONSE GET /swap/v2/build — HTTP status received"
    );

    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(anyhow!(
            "GET /build HTTP {}: {}",
            status,
            &body[..body.len().min(400)]
        ));
    }

    let raw: BuildResponse = resp.json().await
        .map_err(|e| anyhow!("GET /build parse failed: {e}"))?;

    // ── Validate required fields ──────────────────────────────────────────────

    let swap_instruction = raw.swap_instruction
        .ok_or_else(|| anyhow!("GET /build: swapInstruction missing"))?;

    let blockhash = raw.blockhash
        .ok_or_else(|| anyhow!("GET /build: blockhash missing"))?;

    let compute_budget_instructions = raw.compute_budget_instructions.unwrap_or_default();
    let setup_instructions          = raw.setup_instructions.unwrap_or_default();
    let cleanup_instruction         = raw.cleanup_instruction;
    let other_instructions          = raw.other_instructions.unwrap_or_default();
    let last_valid_block_height     = raw.last_valid_block_height.unwrap_or(0);
    let prioritization_fee_lamports = raw.prioritization_fee_lamports.unwrap_or(0);

    // ── Resolve ALT map ───────────────────────────────────────────────────────
    // If /build returns the full alt_map (addresses_by_lookup_table_address),
    // use it directly.  Otherwise fetch ALT account data from Solana RPC.

    let alt_map: HashMap<String, Vec<String>> = if let Some(map) = raw.addresses_by_lookup_table_address {
        debug!(tables = map.len(), "GET /build: ALT map provided directly by API");
        map
    } else {
        let alt_addresses = raw.address_lookup_table_addresses.unwrap_or_default();
        if alt_addresses.is_empty() {
            debug!("GET /build: no ALTs — static accounts only");
            HashMap::new()
        } else {
            debug!(
                alt_count = alt_addresses.len(),
                "GET /build: fetching ALT account data from Solana RPC"
            );
            fetch_alt_maps(rpc_url, &alt_addresses).await?
        }
    };

    info!(
        input_mint,
        output_mint,
        amount,
        compute_budget_ixs = compute_budget_instructions.len(),
        setup_ixs          = setup_instructions.len(),
        has_cleanup        = cleanup_instruction.is_some(),
        alt_tables         = alt_map.len(),
        blockhash          = %blockhash,
        priority_fee       = prioritization_fee_lamports,
        "GET /build: swap instructions received"
    );

    Ok(BuildInstructions {
        compute_budget_instructions,
        setup_instructions,
        swap_instruction,
        cleanup_instruction,
        other_instructions,
        alt_map,
        blockhash,
        last_valid_block_height,
        prioritization_fee_lamports,
    })
}

// ── ALT resolution via Solana RPC ─────────────────────────────────────────────

/// Fetch the pubkeys stored in each Address Lookup Table via Solana RPC.
///
/// Uses getAccountInfo with base64 encoding, then parses the on-chain account
/// data to extract the ordered list of 32-byte public keys.
async fn fetch_alt_maps(
    rpc_url:      &str,
    alt_addresses: &[String],
) -> Result<HashMap<String, Vec<String>>> {
    let client = Client::builder()
        .timeout(Duration::from_millis(5_000))
        .build()?;

    let mut alt_map: HashMap<String, Vec<String>> = HashMap::new();

    for alt_addr in alt_addresses {
        match fetch_single_alt(&client, rpc_url, alt_addr).await {
            Ok(pubkeys) => {
                debug!(alt = %alt_addr, count = pubkeys.len(), "ALT resolved");
                alt_map.insert(alt_addr.clone(), pubkeys);
            }
            Err(e) => {
                warn!(alt = %alt_addr, error = %e, "ALT fetch failed — skipping this table");
            }
        }
    }

    Ok(alt_map)
}

/// Fetch and parse the pubkeys stored in a single Address Lookup Table.
///
/// ALT account data layout (Solana on-chain format):
///   [4 bytes]  type discriminator (u32 LE, 1 = active)
///   [8 bytes]  deactivation_slot (u64 LE)
///   [8 bytes]  last_extended_slot (u64 LE)
///   [1 byte]   last_extended_slot_start_index
///   [1 byte]   has_authority (0 or 1)
///   [32 bytes] authority pubkey (only if has_authority == 1)
///   [2 bytes]  padding
///   [32*n]     pubkeys stored in this ALT
async fn fetch_single_alt(
    client:   &Client,
    rpc_url:  &str,
    alt_addr: &str,
) -> Result<Vec<String>> {
    #[derive(Serialize)]
    struct RpcRequest<'a> {
        jsonrpc: &'static str,
        id:      u64,
        method:  &'a str,
        params:  serde_json::Value,
    }

    #[derive(Deserialize, Debug)]
    struct RpcResponse {
        result: Option<serde_json::Value>,
        error:  Option<serde_json::Value>,
    }

    let request = RpcRequest {
        jsonrpc: "2.0",
        id:      1,
        method:  "getAccountInfo",
        params:  serde_json::json!([
            alt_addr,
            {"encoding": "base64", "commitment": "confirmed"}
        ]),
    };

    let resp = client
        .post(rpc_url)
        .json(&request)
        .send()
        .await
        .context("getAccountInfo HTTP failed")?;

    if !resp.status().is_success() {
        return Err(anyhow!("getAccountInfo HTTP {}", resp.status()));
    }

    let parsed: RpcResponse = resp.json().await.context("getAccountInfo parse failed")?;

    if let Some(err) = parsed.error {
        return Err(anyhow!("getAccountInfo RPC error: {err}"));
    }

    let result = parsed.result
        .ok_or_else(|| anyhow!("getAccountInfo: missing result"))?;

    // Extract base64 account data
    let data_b64 = result
        .get("value")
        .and_then(|v| v.get("data"))
        .and_then(|d| d.as_array())
        .and_then(|arr| arr.first())
        .and_then(|s| s.as_str())
        .ok_or_else(|| anyhow!("getAccountInfo: missing account data for ALT {}", alt_addr))?;

    let raw_data = base64::engine::general_purpose::STANDARD
        .decode(data_b64)
        .context("ALT account data: base64 decode failed")?;

    parse_alt_pubkeys(&raw_data)
        .with_context(|| format!("Failed to parse ALT account data for {alt_addr}"))
}

/// Parse the ordered list of pubkeys from raw ALT account data.
fn parse_alt_pubkeys(data: &[u8]) -> Result<Vec<String>> {
    // Minimum header:
    //   4 (discriminator) + 8 (deact_slot) + 8 (last_ext_slot) +
    //   1 (start_index) + 1 (has_authority) + 2 (padding) = 24 bytes
    if data.len() < 24 {
        return Err(anyhow!("ALT data too short: {} bytes", data.len()));
    }

    let has_authority = data[21] != 0;
    let pubkeys_offset = if has_authority {
        // 4 + 8 + 8 + 1 + 1 + 32 + 2 = 56
        if data.len() < 56 {
            return Err(anyhow!("ALT data too short for authority: {} bytes", data.len()));
        }
        56usize
    } else {
        24usize
    };

    let remaining = &data[pubkeys_offset..];
    if remaining.len() % 32 != 0 {
        warn!(
            remaining_bytes = remaining.len(),
            "ALT pubkey data length not a multiple of 32 — truncating"
        );
    }

    let pubkeys: Vec<String> = remaining
        .chunks_exact(32)
        .map(|chunk| {
            let mut arr = [0u8; 32];
            arr.copy_from_slice(chunk);
            bs58::encode(arr).into_string()
        })
        .collect();

    Ok(pubkeys)
}

// ── Jito tip instruction ───────────────────────────────────────────────────────

/// Build a SystemProgram::Transfer instruction for a Jito tip.
///
/// The tip account must be write-locked in the transaction.
/// Per Jito spec: tip instruction embedded in the MEV transaction itself.
pub fn build_tip_instruction(
    from_pubkey:  &str,
    tip_account:  &str,
    tip_lamports: u64,
) -> V2Instruction {
    // SystemProgram Transfer discriminator = 2 (u32 LE) + lamports (u64 LE)
    let mut data = vec![2u8, 0, 0, 0];
    data.extend_from_slice(&tip_lamports.to_le_bytes());

    V2Instruction {
        program_id: SYSTEM_PROGRAM.to_string(),
        accounts: vec![
            V2AccountMeta {
                pubkey:      from_pubkey.to_string(),
                is_signer:   true,
                is_writable: true,
            },
            V2AccountMeta {
                pubkey:      tip_account.to_string(),
                is_signer:   false,
                is_writable: true, // must be writable — Jito tip account write-lock requirement
            },
        ],
        data: base64::engine::general_purpose::STANDARD.encode(&data),
    }
}
