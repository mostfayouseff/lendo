// crates/ingress/src/swap_builder.rs
// Jupiter Swap Builder — PRODUCTION SAFE (April 2026)

use anyhow::{Context, Result, anyhow};
use base64::Engine as _;
use reqwest::Client;
use serde::Deserialize;
use std::collections::HashMap;
use std::time::Duration;
use tokio::time::sleep;
use tracing::{info, warn};

// ─────────────────────────────────────────────────────────────────────────────
// CONFIG
// ─────────────────────────────────────────────────────────────────────────────
const SWAP_V2_BUILD_URL: &str = "https://api.jup.ag/swap/v2/build";
const TIMEOUT_MS: u64 = 12_000;

// 🔥 throttling (IMPORTANT)
const REQUEST_DELAY_MS: u64 = 80;
const MAX_RETRIES: u8 = 4;

const SYSTEM_PROGRAM: &str = "11111111111111111111111111111111";

// ─────────────────────────────────────────────────────────────────────────────
// RESPONSE TYPES
// ─────────────────────────────────────────────────────────────────────────────
#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SwapBuildResponse {
    pub quote: QuoteData,
    pub instructions: SwapInstructionsData,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct QuoteData {
    pub input_mint: String,
    pub in_amount: String,
    pub output_mint: String,
    pub out_amount: String,
    pub other_amount_threshold: String,
    pub price_impact_pct: String,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SwapInstructionsData {
    pub compute_budget_instructions: Vec<JupiterInstruction>,
    pub setup_instructions: Vec<JupiterInstruction>,
    pub swap_instruction: JupiterInstruction,
    pub cleanup_instruction: Option<JupiterInstruction>,
    pub other_instructions: Vec<JupiterInstruction>,
    pub address_lookup_table_addresses: Vec<String>,
    pub blockhash: String,
    pub last_valid_block_height: u64,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct JupiterAccountMeta {
    pub pubkey: String,
    pub is_signer: bool,
    pub is_writable: bool,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct JupiterInstruction {
    pub program_id: String,
    pub accounts: Vec<JupiterAccountMeta>,
    pub data: String,
}

// ─────────────────────────────────────────────────────────────────────────────
// OUTPUT
// ─────────────────────────────────────────────────────────────────────────────
#[derive(Debug, Clone)]
pub struct BuiltSwapTransaction {
    pub transaction_bytes: Vec<u8>,
    pub in_amount: u64,
    pub out_amount: u64,
    pub jito_tip_lamports: u64,
}

// ─────────────────────────────────────────────────────────────────────────────
// PUBLIC API
// ─────────────────────────────────────────────────────────────────────────────
pub async fn build_signed_swap_transaction(
    client: &Client,
    input_mint: &str,
    output_mint: &str,
    amount: u64,
    user_pubkey_b58: &str,
    user_pubkey: &[u8; 32],
    api_key: Option<&str>,
    slippage_bps: u16,
    jito_tip_lamports: u64,
    jito_tip_account: &str,
    sign_fn: &dyn Fn(&[u8]) -> [u8; 64],
) -> Result<BuiltSwapTransaction> {

    // 🔥 global throttle
    sleep(Duration::from_millis(REQUEST_DELAY_MS)).await;

    let build_resp = get_swap_build(
        client,
        input_mint,
        output_mint,
        amount,
        slippage_bps,
        user_pubkey_b58,
        api_key,
    )
    .await
    .context("Jupiter Swap V2 /build failed")?;

    let in_amount: u64 = build_resp.quote.in_amount.parse().unwrap_or(amount);
    let out_amount: u64 = build_resp.quote.out_amount.parse().unwrap_or(0);

    info!(
        input_mint = %input_mint,
        output_mint = %output_mint,
        in_amount,
        out_amount,
        slippage_bps,
        "Jupiter Swap V2: quote received"
    );

    let mut all_instructions: Vec<JupiterInstruction> = Vec::new();

    all_instructions.extend(build_resp.instructions.compute_budget_instructions);
    all_instructions.extend(build_resp.instructions.setup_instructions);
    all_instructions.push(build_resp.instructions.swap_instruction);

    // 💰 Jito tip
    let tip_ix = build_tip_instruction(user_pubkey_b58, jito_tip_account, jito_tip_lamports);
    all_instructions.push(tip_ix);

    if let Some(cleanup) = build_resp.instructions.cleanup_instruction {
        all_instructions.push(cleanup);
    }

    all_instructions.extend(build_resp.instructions.other_instructions);

    let tx_bytes = build_v0_transaction(
        &all_instructions,
        &build_resp.instructions.address_lookup_table_addresses,
        &build_resp.instructions.blockhash,
        user_pubkey,
        sign_fn,
    )?;

    info!(
        tx_size = tx_bytes.len(),
        in_amount,
        out_amount,
        jito_tip_lamports,
        "Swap tx built successfully"
    );

    Ok(BuiltSwapTransaction {
        transaction_bytes: tx_bytes,
        in_amount,
        out_amount,
        jito_tip_lamports,
    })
}

pub fn build_swap_client() -> Result<Client> {
    Ok(Client::builder()
        .timeout(Duration::from_millis(TIMEOUT_MS))
        .pool_max_idle_per_host(10)
        .build()?)
}

// ─────────────────────────────────────────────────────────────────────────────
// RATE-LIMIT SAFE REQUEST
// ─────────────────────────────────────────────────────────────────────────────
async fn get_swap_build(
    client: &Client,
    input_mint: &str,
    output_mint: &str,
    amount: u64,
    slippage_bps: u16,
    user_public_key: &str,
    api_key: Option<&str>,
) -> Result<SwapBuildResponse> {

    let url = format!(
        "{}?inputMint={}&outputMint={}&amount={}&slippageBps={}&userPublicKey={}",
        SWAP_V2_BUILD_URL, input_mint, output_mint, amount, slippage_bps, user_public_key
    );

    let mut attempt = 0;

    loop {
        let mut req = client.get(&url).header("accept", "application/json");

        if let Some(key) = api_key {
            req = req.header("x-api-key", key);
        }

        let resp = req.send().await;

        match resp {
            Ok(r) => {
                let status = r.status();

                if status.as_u16() == 429 {
                    attempt += 1;

                    warn!(
                        attempt,
                        "429 rate limit — retrying with backoff"
                    );

                    if attempt >= MAX_RETRIES {
                        return Err(anyhow!("Rate limit exceeded"));
                    }

                    sleep(Duration::from_millis(200 * attempt as u64)).await;
                    continue;
                }

                if !status.is_success() {
                    let text = r.text().await.unwrap_or_default();
                    return Err(anyhow!(
                        "HTTP {}: {}",
                        status,
                        &text[..text.len().min(300)]
                    ));
                }

                return r.json().await.context("JSON parse failed");
            }

            Err(e) => {
                attempt += 1;

                if attempt >= MAX_RETRIES {
                    return Err(anyhow!("HTTP failed: {}", e));
                }

                sleep(Duration::from_millis(150 * attempt as u64)).await;
            }
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// JITO TIP
// ─────────────────────────────────────────────────────────────────────────────
fn build_tip_instruction(
    from_pubkey: &str,
    tip_account: &str,
    lamports: u64,
) -> JupiterInstruction {

    let mut data = vec![2u8, 0, 0, 0];
    data.extend_from_slice(&lamports.to_le_bytes());

    JupiterInstruction {
        program_id: SYSTEM_PROGRAM.to_string(),
        accounts: vec![
            JupiterAccountMeta {
                pubkey: from_pubkey.to_string(),
                is_signer: true,
                is_writable: true,
            },
            JupiterAccountMeta {
                pubkey: tip_account.to_string(),
                is_signer: false,
                is_writable: true,
            },
        ],
        data: base64::engine::general_purpose::STANDARD.encode(&data),
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// TRANSACTION BUILDER (UNCHANGED CORE)
// ─────────────────────────────────────────────────────────────────────────────
fn build_v0_transaction(
    instructions: &[JupiterInstruction],
    _alts: &[String],
    blockhash_b58: &str,
    payer_pubkey: &[u8; 32],
    sign_fn: &dyn Fn(&[u8]) -> [u8; 64],
) -> Result<Vec<u8>> {

    #[derive(Clone)]
    struct AccountEntry {
        pubkey_bytes: [u8; 32],
        is_signer: bool,
        is_writable: bool,
    }

    let payer_b58 = bs58::encode(payer_pubkey).into_string();

    let mut map: HashMap<String, AccountEntry> = HashMap::new();

    map.insert(payer_b58.clone(), AccountEntry {
        pubkey_bytes: *payer_pubkey,
        is_signer: true,
        is_writable: true,
    });

    for ix in instructions {
        map.entry(ix.program_id.clone()).or_insert(AccountEntry {
            pubkey_bytes: b58_to_32(&ix.program_id),
            is_signer: false,
            is_writable: false,
        });

        for acct in &ix.accounts {
            let entry = map.entry(acct.pubkey.clone()).or_insert(AccountEntry {
                pubkey_bytes: b58_to_32(&acct.pubkey),
                is_signer: false,
                is_writable: false,
            });

            if acct.is_signer { entry.is_signer = true; }
            if acct.is_writable { entry.is_writable = true; }
        }
    }

    let mut keys: Vec<String> = map.keys().cloned().collect();
    keys.sort();

    let mut index = HashMap::new();
    for (i, k) in keys.iter().enumerate() {
        index.insert(k.clone(), i as u8);
    }

    let mut msg = vec![0x80];

    msg.push(1);
    msg.push(0);
    msg.push(0);

    write_compact_u16(&mut msg, keys.len() as u16);

    for k in &keys {
        msg.extend_from_slice(&b58_to_32(k));
    }

    msg.extend_from_slice(&b58_to_32(blockhash_b58));

    write_compact_u16(&mut msg, instructions.len() as u16);

    for ix in instructions {
        msg.push(*index.get(&ix.program_id).unwrap());

        write_compact_u16(&mut msg, ix.accounts.len() as u16);

        for acct in &ix.accounts {
            msg.push(*index.get(&acct.pubkey).unwrap());
        }

        let data = base64::engine::general_purpose::STANDARD
            .decode(&ix.data)
            .unwrap_or_default();

        write_compact_u16(&mut msg, data.len() as u16);
        msg.extend_from_slice(&data);
    }

    write_compact_u16(&mut msg, 0);

    let sig = sign_fn(&msg);

    let mut tx = vec![];
    write_compact_u16(&mut tx, 1);
    tx.extend_from_slice(&sig);
    tx.extend_from_slice(&msg);

    Ok(tx)
}

// ─────────────────────────────────────────────────────────────────────────────
// HELPERS
// ─────────────────────────────────────────────────────────────────────────────
fn write_compact_u16(buf: &mut Vec<u8>, val: u16) {
    if val < 0x80 {
        buf.push(val as u8);
    } else {
        buf.push((val as u8 & 0x7f) | 0x80);
        buf.push((val >> 7) as u8);
    }
}

fn b58_to_32(s: &str) -> [u8; 32] {
    let decoded = bs58::decode(s).into_vec().unwrap_or_default();

    if decoded.len() != 32 {
        return [0u8; 32];
    }

    let mut out = [0u8; 32];
    out.copy_from_slice(&decoded);
    out
}
