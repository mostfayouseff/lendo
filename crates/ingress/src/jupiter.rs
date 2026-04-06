// crates/ingress/src/swap_builder.rs
// Jupiter Swap Builder - Fixed for v1 API (2026)

use anyhow::{Context, Result, anyhow};
use base64::Engine as _;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Duration;
use tracing::{debug, info, warn};

const QUOTE_URL: &str = "https://api.jup.ag/swap/v1/quote";
const SWAP_INSTRUCTIONS_URL: &str = "https://api.jup.ag/swap/v1/swap-instructions";
const TIMEOUT_MS: u64 = 12_000;

const SYSTEM_PROGRAM: &str = "11111111111111111111111111111111";

// ── Quote Response ───────────────────────────────────────────────────────────
#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct QuoteResponse {
    pub input_mint: String,
    pub in_amount: String,
    pub output_mint: String,
    pub out_amount: String,
    pub other_amount_threshold: String,
    pub swap_mode: String,
    pub slippage_bps: u64,
    pub price_impact_pct: String,
    pub route_plan: Vec<serde_json::Value>,
}

// ── Swap Instructions Response ───────────────────────────────────────────────
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
    pub data: String, // base64
}

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SwapInstructionsResponse {
    pub compute_budget_instructions: Vec<JupiterInstruction>,
    pub setup_instructions: Vec<JupiterInstruction>,
    pub swap_instruction: JupiterInstruction,
    pub cleanup_instruction: Option<JupiterInstruction>,
    pub other_instructions: Vec<JupiterInstruction>,
    pub address_lookup_table_addresses: Vec<String>,
    pub blockhash: String,
    pub last_valid_block_height: u64,
    pub prioritization_fee_lamports: Option<u64>,
}

// ── Request Body ─────────────────────────────────────────────────────────────
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SwapInstructionsRequest<'a> {
    quote_response: &'a QuoteResponse,
    user_public_key: &'a str,
    wrap_and_unwrap_sol: bool,
    use_shared_accounts: bool,
    dynamic_compute_unit_limit: bool,
    skip_user_accounts_rpc_calls: bool,
    use_token_ledger: bool,
}

// ── Output ───────────────────────────────────────────────────────────────────
#[derive(Debug, Clone)]
pub struct BuiltSwapTransaction {
    pub transaction_bytes: Vec<u8>,
    pub in_amount: u64,
    pub out_amount: u64,
    pub jito_tip_lamports: u64,
}

// ── Public API ───────────────────────────────────────────────────────────────
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
    let quote = get_quote(client, input_mint, output_mint, amount, slippage_bps, api_key)
        .await
        .context("Jupiter /swap/v1/quote failed")?;

    let in_amount: u64 = quote.in_amount.parse().unwrap_or(amount);
    let out_amount: u64 = quote.out_amount.parse().unwrap_or(0);

    info!(
        input_mint = %input_mint,
        output_mint = %output_mint,
        in_amount,
        out_amount,
        slippage_bps,
        "Jupiter quote received successfully"
    );

    let swap_ix_resp = get_swap_instructions(client, &quote, user_pubkey_b58, api_key)
        .await
        .context("Jupiter /swap/v1/swap-instructions failed")?;

    info!(
        compute_ixs = swap_ix_resp.compute_budget_instructions.len(),
        setup_ixs = swap_ix_resp.setup_instructions.len(),
        has_cleanup = swap_ix_resp.cleanup_instruction.is_some(),
        alts = swap_ix_resp.address_lookup_table_addresses.len(),
        blockhash = %swap_ix_resp.blockhash,
        "Jupiter swap instructions received"
    );

    let mut all_instructions: Vec<JupiterInstruction> = Vec::new();
    all_instructions.extend(swap_ix_resp.compute_budget_instructions);
    all_instructions.extend(swap_ix_resp.setup_instructions);
    all_instructions.push(swap_ix_resp.swap_instruction);

    // Jito tip
    let tip_ix = build_tip_instruction(user_pubkey_b58, jito_tip_account, jito_tip_lamports);
    all_instructions.push(tip_ix);

    if let Some(cleanup) = swap_ix_resp.cleanup_instruction {
        all_instructions.push(cleanup);
    }
    all_instructions.extend(swap_ix_resp.other_instructions);

    let tx_bytes = build_v0_transaction(
        &all_instructions,
        &swap_ix_resp.address_lookup_table_addresses,
        &swap_ix_resp.blockhash,
        user_pubkey,
        sign_fn,
    )
    .context("Failed to build v0 versioned transaction")?;

    info!(
        tx_size = tx_bytes.len(),
        in_amount,
        out_amount,
        jito_tip_lamports,
        "Jupiter swap transaction built and signed (v0)"
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
        .build()?)
}

// ── Internal Helpers ─────────────────────────────────────────────────────────
async fn get_quote(
    client: &Client,
    input_mint: &str,
    output_mint: &str,
    amount: u64,
    slippage_bps: u16,
    api_key: Option<&str>,
) -> Result<QuoteResponse> {
    let url = format!(
        "{}?inputMint={}&outputMint={}&amount={}&slippageBps={}&onlyDirectRoutes=false",
        QUOTE_URL, input_mint, output_mint, amount, slippage_bps
    );

    let mut req = client.get(&url).header("accept", "application/json");
    if let Some(key) = api_key {
        req = req.header("x-api-key", key);
    }

    let resp = req.send().await.context("Quote HTTP request failed")?;
    let status = resp.status();

    if status.as_u16() == 429 {
        warn!("Jupiter quote rate limit hit (429)");
    }
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(anyhow!(
            "Jupiter quote HTTP {}: {}",
            status,
            &body[..body.len().min(500)]
        ));
    }

    resp.json().await.context("Failed to parse quote")
}

async fn get_swap_instructions(
    client: &Client,
    quote: &QuoteResponse,
    user_pubkey: &str,
    api_key: Option<&str>,
) -> Result<SwapInstructionsResponse> {
    let body = SwapInstructionsRequest {
        quote_response: quote,
        user_public_key: user_pubkey,
        wrap_and_unwrap_sol: true,
        use_shared_accounts: true,
        dynamic_compute_unit_limit: true,
        skip_user_accounts_rpc_calls: true,
        use_token_ledger: false,
    };

    let mut req = client
        .post(SWAP_INSTRUCTIONS_URL)
        .header("accept", "application/json")
        .header("content-type", "application/json")
        .json(&body);

    if let Some(key) = api_key {
        req = req.header("x-api-key", key);
    }

    let resp = req.send().await.context("Swap-instructions HTTP request failed")?;
    let status = resp.status();

    if status.as_u16() == 429 {
        warn!("Jupiter swap-instructions rate limit hit (429)");
    }
    if !status.is_success() {
        let text = resp.text().await.unwrap_or_default();
        return Err(anyhow!(
            "Jupiter swap-instructions HTTP {}: {}",
            status,
            &text[..text.len().min(500)]
        ));
    }

    resp.json().await.context("Failed to parse swap-instructions")
}

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
            JupiterAccountMeta { pubkey: from_pubkey.to_string(), is_signer: true, is_writable: true },
            JupiterAccountMeta { pubkey: tip_account.to_string(), is_signer: false, is_writable: true },
        ],
        data: base64::engine::general_purpose::STANDARD.encode(&data),
    }
}

fn build_v0_transaction(
    instructions: &[JupiterInstruction],
    _address_lookup_table_addresses: &[String],
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
    let mut account_map: HashMap<String, AccountEntry> = HashMap::new();

    account_map.insert(
        payer_b58.clone(),
        AccountEntry { pubkey_bytes: *payer_pubkey, is_signer: true, is_writable: true },
    );

    for ix in instructions {
        account_map.entry(ix.program_id.clone()).or_insert_with(|| AccountEntry {
            pubkey_bytes: b58_to_32(&ix.program_id),
            is_signer: false,
            is_writable: false,
        });

        for acct in &ix.accounts {
            let entry = account_map.entry(acct.pubkey.clone()).or_insert_with(|| AccountEntry {
                pubkey_bytes: b58_to_32(&acct.pubkey),
                is_signer: false,
                is_writable: false,
            });
            if acct.is_signer { entry.is_signer = true; }
            if acct.is_writable { entry.is_writable = true; }
        }
    }

    let mut sw_signers: Vec<String> = vec![payer_b58.clone()];
    let mut ro_signers: Vec<String> = Vec::new();
    let mut sw_static: Vec<String> = Vec::new();
    let mut ro_static: Vec<String> = Vec::new();

    let mut all_pubkeys: Vec<String> = account_map.keys().cloned().collect();
    all_pubkeys.sort();

    for pk in &all_pubkeys {
        if pk == &payer_b58 { continue; }
        let entry = &account_map[pk];
        if entry.is_signer {
            if entry.is_writable {
                sw_signers.push(pk.clone());
            } else {
                ro_signers.push(pk.clone());
            }
        } else if entry.is_writable {
            sw_static.push(pk.clone());
        } else {
            ro_static.push(pk.clone());
        }
    }

    let mut static_accounts: Vec<String> = Vec::new();
    static_accounts.extend(sw_signers.iter().cloned());
    static_accounts.extend(ro_signers.iter().cloned());
    static_accounts.extend(sw_static.iter().cloned());
    static_accounts.extend(ro_static.iter().cloned());

    let mut combined_index: HashMap<String, u8> = HashMap::new();
    for (idx, pk) in static_accounts.iter().enumerate() {
        combined_index.insert(pk.clone(), idx as u8);
    }

    let mut msg: Vec<u8> = Vec::new();
    msg.push(0x80u8);

    let num_required_sigs = (sw_signers.len() + ro_signers.len()) as u8;
    let num_readonly_signed = ro_signers.len() as u8;
    let num_readonly_unsigned = ro_static.len() as u8;

    msg.push(num_required_sigs);
    msg.push(num_readonly_signed);
    msg.push(num_readonly_unsigned);

    write_compact_u16(&mut msg, static_accounts.len() as u16);
    for pk_b58 in &static_accounts {
        msg.extend_from_slice(&b58_to_32(pk_b58));
    }
    msg.extend_from_slice(&b58_to_32(blockhash_b58));

    write_compact_u16(&mut msg, instructions.len() as u16);
    for ix in instructions {
        let prog_idx = *combined_index.get(&ix.program_id).unwrap_or(&0);
        msg.push(prog_idx);

        write_compact_u16(&mut msg, ix.accounts.len() as u16);
        for acct in &ix.accounts {
            let acct_idx = *combined_index.get(&acct.pubkey).unwrap_or(&0);
            msg.push(acct_idx);
        }

        let data = base64::engine::general_purpose::STANDARD
            .decode(&ix.data)
            .unwrap_or_default();
        write_compact_u16(&mut msg, data.len() as u16);
        msg.extend_from_slice(&data);
    }

    write_compact_u16(&mut msg, 0); // No ALTs

    let payer_sig = sign_fn(&msg);
    let mut tx: Vec<u8> = Vec::with_capacity(3 + 64 * num_required_sigs as usize + msg.len());

    write_compact_u16(&mut tx, num_required_sigs as u16);
    tx.extend_from_slice(&payer_sig);
    for _ in 1..num_required_sigs {
        tx.extend_from_slice(&[0u8; 64]);
    }
    tx.extend_from_slice(&msg);

    Ok(tx)
}

fn write_compact_u16(buf: &mut Vec<u8>, val: u16) {
    if val < 0x80 {
        buf.push(val as u8);
    } else if val < 0x4000 {
        buf.push((val as u8 & 0x7f) | 0x80);
        buf.push((val >> 7) as u8);
    } else {
        buf.push((val as u8 & 0x7f) | 0x80);
        buf.push(((val >> 7) as u8 & 0x7f) | 0x80);
        buf.push((val >> 14) as u8);
    }
}

fn b58_to_32(s: &str) -> [u8; 32] {
    let decoded = bs58::decode(s).into_vec().unwrap_or_default();
    if decoded.len() != 32 {
        warn!(addr = %s, "b58_to_32: invalid length");
        return [0u8; 32];
    }
    let mut out = [0u8; 32];
    out.copy_from_slice(&decoded);
    out
}
