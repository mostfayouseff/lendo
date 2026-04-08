// =============================================================================
// ATOMIC FLASH LOAN TRANSACTION BUILDER — VersionedTransaction v0
//
// Builds a fully atomic Solana v0 transaction containing:
//
//   [ComputeBudget]   — priority fee instructions (from Jupiter /build)
//   [FlashLoanBorrow] — Solend FlashBorrowReserveLiquidity
//   [Swap1...]        — Jupiter /build swap instructions (leg 1: SOL → token)
//   [Swap2...]        — Jupiter /build swap instructions (leg 2: token → SOL)
//   [FlashLoanRepay]  — Solend FlashRepayReserveLiquidity
//   [JitoTip]         — SystemProgram::Transfer to Jito tip account
//
// Atomicity: if FlashRepay fails (profit insufficient), the entire tx reverts.
// The Solana runtime enforces this unconditionally.
//
// Transaction format: VersionedTransaction v0 (NOT legacy).
//   • 0x80 version prefix byte
//   • Supports Address Lookup Tables for account compression
//
// Solend mainnet addresses (verified):
//   Program:     So1endDq2YkqhipRh3WViPa8hdiSpxWy6z3Z6tMCpAo
//   Main pool:   4UpD2fh7xH3VP9QQaXtsS1YY3bxzWhtfpks7FatyKvdY
//   SOL reserve: FzbfXR7sopQL29Ubu312tkqWMxSre4dYSrFyYAjUYiC
//   Liq supply:  8UviNr47S8eL6J3WfDxMRa3hvLta1VDJwNAqwTKZcZvj
//   Fee recv:    AXuN52TrDFhw9S8V3gfJvAX2JKK4y9ZtVGnE27PaLqQ
//
// FORBIDDEN: POST /execute  |  mock/placeholder accounts  |  legacy transactions
// =============================================================================

use crate::{
    flash_tx::{b58_to_32, find_associated_token_account, find_program_address, write_compact_u16},
    keypair::ApexKeypair,
};
use anyhow::{anyhow, Result};
use std::collections::HashMap;
use tracing::{debug, info, warn};

// ── Solend mainnet constants ───────────────────────────────────────────────────

const SOLEND_PROGRAM_ID:     &str = "So1endDq2YkqhipRh3WViPa8hdiSpxWy6z3Z6tMCpAo";
const SOLEND_MAIN_POOL:      &str = "4UpD2fh7xH3VP9QQaXtsS1YY3bxzWhtfpks7FatyKvdY";
const SOLEND_SOL_RESERVE:    &str = "FzbfXR7sopQL29Ubu312tkqWMxSre4dYSrFyYAjUYiC";
const SOLEND_SOL_LIQ_SUPPLY: &str = "8UviNr47S8eL6J3WfDxMRa3hvLta1VDJwNAqwTKZcZvj";
const SOLEND_FEE_RECEIVER:   &str = "AXuN52TrDFhw9S8V3gfJvAX2JKK4y9ZtVGnE27PaLqQ";
const TOKEN_PROGRAM_ID:      &str = "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA";
const WSOL_MINT:             &str = "So11111111111111111111111111111111111111112";
const SYSVAR_INSTRUCTIONS:   &str = "Sysvar1nstructions1111111111111111111111111";
const SYSTEM_PROGRAM:        &str = "11111111111111111111111111111111";

const BORROW_DISCRIMINATOR: u8 = 0x14;
const REPAY_DISCRIMINATOR:  u8 = 0x15;

// ── Instruction types ─────────────────────────────────────────────────────────

/// Account reference for an atomic instruction.
/// Mirrors `V2AccountMeta` from the ingress crate without creating a dependency.
#[derive(Debug, Clone)]
pub struct AtomicAccountMeta {
    pub pubkey:      String,
    pub is_signer:   bool,
    pub is_writable: bool,
}

/// A single instruction to inject into the atomic transaction.
#[derive(Debug, Clone)]
pub struct AtomicInstruction {
    pub program_id: String,
    pub accounts:   Vec<AtomicAccountMeta>,
    /// Raw (decoded) instruction data bytes.
    pub data:       Vec<u8>,
}

// ── Internal types ────────────────────────────────────────────────────────────

struct IxEntry {
    program_id: String,
    /// (pubkey_b58, is_signer, is_writable)
    accounts:   Vec<(String, bool, bool)>,
    data:       Vec<u8>,
}

#[derive(Clone)]
struct AccEntry {
    is_signer:   bool,
    is_writable: bool,
}

// ── Public API ─────────────────────────────────────────────────────────────────

/// Build and sign a fully atomic VersionedTransaction v0.
///
/// Instruction order (deterministic):
///   [ComputeBudget...]  — from Jupiter /build
///   [FlashLoanBorrow]   — Solend FlashBorrowReserveLiquidity
///   [Swap1...]          — setup + swap, leg 1 (from Jupiter /build)
///   [Swap2...]          — setup + swap, leg 2 (from Jupiter /build)
///   [FlashLoanRepay]    — Solend FlashRepayReserveLiquidity
///   [JitoTip]           — SystemProgram::Transfer to Jito tip account
///
/// If /build fails for either swap leg, the caller MUST NOT call this function
/// — skip the loop iteration instead.
///
/// # Arguments
/// * `keypair`              — operator signing keypair (payer)
/// * `blockhash_b58`        — recent blockhash from Solana RPC
/// * `borrow_lamports`      — amount to borrow (principal)
/// * `repay_lamports`       — amount to repay = borrow + Solend 0.09% fee
/// * `compute_budget_ixs`   — ComputeBudget instructions from /build
/// * `swap1_ixs`            — swap leg 1 instructions from /build (no ComputeBudget)
/// * `swap2_ixs`            — swap leg 2 instructions from /build (no ComputeBudget)
/// * `alt_map`              — ALT address → ordered pubkeys
/// * `jito_tip_lamports`    — lamports to send as Jito tip (0 = no tip instruction)
/// * `jito_tip_account_b58` — one of the 8 canonical Jito tip accounts
///
/// # Returns
/// Signed, serialized v0 transaction bytes ready for Jito submission and
/// simulation via `SolanaRpcClient::simulate_transaction`.
#[allow(clippy::too_many_arguments)]
pub fn build_atomic_flash_v0(
    keypair:              &ApexKeypair,
    blockhash_b58:        &str,
    borrow_lamports:      u64,
    repay_lamports:       u64,
    compute_budget_ixs:   &[AtomicInstruction],
    swap1_ixs:            &[AtomicInstruction],
    swap2_ixs:            &[AtomicInstruction],
    alt_map:              &HashMap<String, Vec<String>>,
    jito_tip_lamports:    u64,
    jito_tip_account_b58: &str,
) -> Result<Vec<u8>> {
    if repay_lamports <= borrow_lamports {
        return Err(anyhow!(
            "repay_lamports ({}) must exceed borrow_lamports ({}) — Solend 0.09% fee not covered",
            repay_lamports,
            borrow_lamports
        ));
    }
    if swap1_ixs.is_empty() {
        warn!("build_atomic_flash_v0: no swap1 instructions — transaction may not be profitable");
    }

    info!(
        borrow_sol     = format!("{:.6}", borrow_lamports as f64 / 1e9),
        repay_sol      = format!("{:.6}", repay_lamports  as f64 / 1e9),
        compute_budget = compute_budget_ixs.len(),
        swap1_count    = swap1_ixs.len(),
        swap2_count    = swap2_ixs.len(),
        alt_tables     = alt_map.len(),
        tip_lamports   = jito_tip_lamports,
        "flash_tx_v2: assembling atomic VersionedTransaction v0"
    );

    let operator_b58   = keypair.pubkey_b58.clone();
    let operator_bytes = keypair.pubkey_bytes;

    // ── Derive Solend accounts ────────────────────────────────────────────────

    let (lending_market_authority, _) = find_program_address(
        &[&b58_to_32(SOLEND_MAIN_POOL)],
        &b58_to_32(SOLEND_PROGRAM_ID),
    );
    let (borrower_wsol_ata, _) = find_associated_token_account(
        &operator_bytes,
        &b58_to_32(WSOL_MINT),
    );

    let lma_b58  = bs58::encode(lending_market_authority).into_string();
    let ata_b58  = bs58::encode(borrower_wsol_ata).into_string();

    debug!(
        lma  = %lma_b58,
        ata  = %ata_b58,
        op   = %operator_b58,
        "Solend PDAs derived"
    );

    // ── Assemble instruction list ─────────────────────────────────────────────

    let mut ixs: Vec<IxEntry> = Vec::new();

    // [1] ComputeBudget (from /build)
    for ix in compute_budget_ixs {
        ixs.push(from_atomic(ix));
    }

    let borrow_ix_index = ixs.len() as u8; // index FlashBorrow will have in the tx

    // [2] FlashBorrow
    let mut borrow_data = vec![BORROW_DISCRIMINATOR];
    borrow_data.extend_from_slice(&borrow_lamports.to_le_bytes());

    ixs.push(IxEntry {
        program_id: SOLEND_PROGRAM_ID.to_string(),
        accounts: vec![
            (SOLEND_SOL_LIQ_SUPPLY.to_string(), false, true),  // source_liquidity
            (ata_b58.clone(),                   false, true),  // destination_liquidity
            (SOLEND_SOL_RESERVE.to_string(),    false, true),  // reserve
            (SOLEND_FEE_RECEIVER.to_string(),   false, true),  // fee_receiver
            (SOLEND_FEE_RECEIVER.to_string(),   false, true),  // host_fee_receiver
            (SOLEND_MAIN_POOL.to_string(),      false, false), // lending_market
            (lma_b58.clone(),                   false, true),  // lending_market_authority
            (TOKEN_PROGRAM_ID.to_string(),      false, false), // token_program
            (SYSVAR_INSTRUCTIONS.to_string(),   false, false), // sysvar_instructions
        ],
        data: borrow_data,
    });

    info!(borrow_lamports, discriminator = BORROW_DISCRIMINATOR, "FlashBorrow ix built");

    // [3] Swap leg 1 (all instructions from /build, excluding ComputeBudget)
    for ix in swap1_ixs {
        ixs.push(from_atomic(ix));
    }

    // [4] Swap leg 2
    for ix in swap2_ixs {
        ixs.push(from_atomic(ix));
    }

    // [5] FlashRepay
    let mut repay_data = vec![REPAY_DISCRIMINATOR];
    repay_data.extend_from_slice(&repay_lamports.to_le_bytes());
    repay_data.push(borrow_ix_index); // index of the FlashBorrow in this transaction

    ixs.push(IxEntry {
        program_id: SOLEND_PROGRAM_ID.to_string(),
        accounts: vec![
            (ata_b58.clone(),                   false, true),  // source_liquidity (repay from)
            (SOLEND_SOL_LIQ_SUPPLY.to_string(), false, true),  // destination_liquidity
            (SOLEND_SOL_RESERVE.to_string(),    false, true),  // reserve
            (SOLEND_FEE_RECEIVER.to_string(),   false, true),  // fee_receiver
            (SOLEND_FEE_RECEIVER.to_string(),   false, true),  // host_fee_receiver
            (SOLEND_MAIN_POOL.to_string(),      false, false), // lending_market
            (lma_b58.clone(),                   false, true),  // lending_market_authority
            (operator_b58.clone(),              true,  true),  // user_transfer_authority (SIGNER)
            (TOKEN_PROGRAM_ID.to_string(),      false, false), // token_program
            (SYSVAR_INSTRUCTIONS.to_string(),   false, false), // sysvar_instructions
        ],
        data: repay_data,
    });

    info!(
        repay_lamports,
        discriminator  = REPAY_DISCRIMINATOR,
        borrow_ix_index,
        "FlashRepay ix built"
    );

    // [6] Jito tip (embedded in same tx — uncle-bandit protection)
    if jito_tip_lamports > 0 {
        let mut tip_data = vec![2u8, 0, 0, 0]; // SystemProgram Transfer
        tip_data.extend_from_slice(&jito_tip_lamports.to_le_bytes());

        ixs.push(IxEntry {
            program_id: SYSTEM_PROGRAM.to_string(),
            accounts: vec![
                (operator_b58.clone(),              true,  true),
                (jito_tip_account_b58.to_string(),  false, true),
            ],
            data: tip_data,
        });

        debug!(
            tip_lamports = jito_tip_lamports,
            tip_account  = %jito_tip_account_b58,
            "Jito tip ix added"
        );
    }

    // ── Assemble and sign the v0 transaction ──────────────────────────────────

    build_v0_message_and_sign(
        &ixs,
        alt_map,
        blockhash_b58,
        &operator_bytes,
        &operator_b58,
        keypair,
    )
}

// ── v0 VersionedTransaction builder ───────────────────────────────────────────

fn build_v0_message_and_sign(
    instructions:  &[IxEntry],
    alt_map:       &HashMap<String, Vec<String>>,
    blockhash_b58: &str,
    payer_bytes:   &[u8; 32],
    payer_b58:     &str,
    keypair:       &ApexKeypair,
) -> Result<Vec<u8>> {
    // ── Collect unique accounts ───────────────────────────────────────────────

    let mut account_map: HashMap<String, AccEntry> = HashMap::new();

    account_map.insert(payer_b58.to_string(), AccEntry {
        is_signer:   true,
        is_writable: true,
    });

    for ix in instructions {
        account_map.entry(ix.program_id.clone()).or_insert_with(|| AccEntry {
            is_signer:   false,
            is_writable: false,
        });
        for (pk, is_signer, is_writable) in &ix.accounts {
            let e = account_map.entry(pk.clone()).or_insert_with(|| AccEntry {
                is_signer:   false,
                is_writable: false,
            });
            if *is_signer   { e.is_signer   = true; }
            if *is_writable { e.is_writable  = true; }
        }
    }

    // ── ALT reverse-lookup ────────────────────────────────────────────────────

    let mut alt_reverse: HashMap<String, (String, u8)> = HashMap::new();
    for (alt_addr, pubkeys) in alt_map {
        for (i, pk) in pubkeys.iter().enumerate() {
            if i < 256 {
                alt_reverse.insert(pk.clone(), (alt_addr.clone(), i as u8));
            }
        }
    }

    // ── Classify accounts ─────────────────────────────────────────────────────
    // Signers must be static; non-signers can be ALT-referenced.

    let mut sw_signers: Vec<String> = vec![payer_b58.to_string()];
    let mut ro_signers: Vec<String> = Vec::new();
    let mut sw_static:  Vec<String> = Vec::new();
    let mut ro_static:  Vec<String> = Vec::new();

    let mut alt_buckets: HashMap<String, Vec<(u8, bool)>> = HashMap::new();

    let mut sorted_pks: Vec<String> = account_map.keys().cloned().collect();
    sorted_pks.sort();

    for pk in &sorted_pks {
        if pk == payer_b58 { continue; }
        let e = &account_map[pk];
        if e.is_signer {
            if e.is_writable { sw_signers.push(pk.clone()); }
            else             { ro_signers.push(pk.clone()); }
        } else if let Some((alt_addr, on_idx)) = alt_reverse.get(pk) {
            alt_buckets.entry(alt_addr.clone()).or_default().push((*on_idx, e.is_writable));
        } else {
            if e.is_writable { sw_static.push(pk.clone()); }
            else             { ro_static.push(pk.clone()); }
        }
    }

    let mut static_accounts: Vec<String> = Vec::new();
    static_accounts.extend(sw_signers.iter().cloned());
    static_accounts.extend(ro_signers.iter().cloned());
    static_accounts.extend(sw_static.iter().cloned());
    static_accounts.extend(ro_static.iter().cloned());

    // ── Combined index map ────────────────────────────────────────────────────

    let mut combined_idx: HashMap<String, u8> = HashMap::new();
    let mut next: u16 = 0;

    for pk in &static_accounts {
        combined_idx.insert(pk.clone(), next as u8);
        next += 1;
    }

    struct AltEntry {
        key_bytes:  [u8; 32],
        wr_indices: Vec<u8>,
        ro_indices: Vec<u8>,
    }

    let mut sorted_alts: Vec<String> = alt_buckets.keys().cloned().collect();
    sorted_alts.sort();

    let mut alt_entries: Vec<AltEntry> = Vec::new();

    for alt_addr in &sorted_alts {
        let slots    = &alt_buckets[alt_addr];
        let accts    = alt_map.get(alt_addr).map(|v| v.as_slice()).unwrap_or(&[]);

        let mut wr: Vec<(u8, String)> = slots.iter().filter(|(_, w)| *w)
            .map(|(i, _)| (*i, accts.get(*i as usize).cloned().unwrap_or_default()))
            .collect();
        let mut ro: Vec<(u8, String)> = slots.iter().filter(|(_, w)| !*w)
            .map(|(i, _)| (*i, accts.get(*i as usize).cloned().unwrap_or_default()))
            .collect();
        wr.sort_by_key(|(i, _)| *i);
        ro.sort_by_key(|(i, _)| *i);

        for (_, pk) in &wr { if !pk.is_empty() { combined_idx.insert(pk.clone(), next as u8); next += 1; } }
        for (_, pk) in &ro { if !pk.is_empty() { combined_idx.insert(pk.clone(), next as u8); next += 1; } }

        alt_entries.push(AltEntry {
            key_bytes:  b58_to_32(alt_addr),
            wr_indices: wr.iter().map(|(i, _)| *i).collect(),
            ro_indices: ro.iter().map(|(i, _)| *i).collect(),
        });
    }

    debug!(
        static_accounts = static_accounts.len(),
        alt_tables      = alt_entries.len(),
        total_accounts  = next,
        signers         = sw_signers.len() + ro_signers.len(),
        "v0 account table built"
    );

    // ── Serialize v0 message ──────────────────────────────────────────────────

    let mut msg: Vec<u8> = Vec::new();

    msg.push(0x80u8); // v0 prefix

    // Header
    let num_req_sigs   = (sw_signers.len() + ro_signers.len()) as u8;
    let num_ro_signed  = ro_signers.len() as u8;
    let num_ro_unsigned = ro_static.len() as u8;
    msg.push(num_req_sigs);
    msg.push(num_ro_signed);
    msg.push(num_ro_unsigned);

    // Static accounts
    write_compact_u16(&mut msg, static_accounts.len() as u16);
    for pk in &static_accounts {
        msg.extend_from_slice(&b58_to_32(pk));
    }

    // Recent blockhash
    msg.extend_from_slice(&b58_to_32(blockhash_b58));

    // Instructions
    write_compact_u16(&mut msg, instructions.len() as u16);
    for ix in instructions {
        let prog_idx = *combined_idx.get(&ix.program_id)
            .ok_or_else(|| anyhow!("Program ID missing from account table: {}", ix.program_id))?;
        msg.push(prog_idx);

        write_compact_u16(&mut msg, ix.accounts.len() as u16);
        for (pk, _, _) in &ix.accounts {
            let acct_idx = *combined_idx.get(pk)
                .ok_or_else(|| anyhow!("Account not in combined table: {pk}"))?;
            msg.push(acct_idx);
        }

        write_compact_u16(&mut msg, ix.data.len() as u16);
        msg.extend_from_slice(&ix.data);
    }

    // Address table lookups
    write_compact_u16(&mut msg, alt_entries.len() as u16);
    for entry in &alt_entries {
        msg.extend_from_slice(&entry.key_bytes);
        write_compact_u16(&mut msg, entry.wr_indices.len() as u16);
        for &i in &entry.wr_indices { msg.push(i); }
        write_compact_u16(&mut msg, entry.ro_indices.len() as u16);
        for &i in &entry.ro_indices { msg.push(i); }
    }

    // ── Sign (message bytes include the 0x80 prefix) ──────────────────────────

    let sig = keypair.sign(&msg);

    debug_assert!(
        keypair.verify(&msg, &sig),
        "Signature self-verification failed"
    );

    let mut tx: Vec<u8> = Vec::with_capacity(1 + 64 + msg.len());
    tx.push(1u8); // compact-u16: 1 signature
    tx.extend_from_slice(&sig);
    tx.extend_from_slice(&msg);

    info!(
        tx_size      = tx.len(),
        ix_count     = instructions.len(),
        static_accts = static_accounts.len(),
        alt_tables   = alt_entries.len(),
        "v0 transaction built and signed"
    );

    Ok(tx)
}

// ── Helpers ────────────────────────────────────────────────────────────────────

fn from_atomic(ix: &AtomicInstruction) -> IxEntry {
    IxEntry {
        program_id: ix.program_id.clone(),
        accounts:   ix.accounts.iter()
            .map(|a| (a.pubkey.clone(), a.is_signer, a.is_writable))
            .collect(),
        data: ix.data.clone(),
    }
}

// ── Fee calculation ────────────────────────────────────────────────────────────

/// Calculate the Solend flash loan repayment amount.
///
/// Solend charges exactly 0.09% (9 bps) per borrow.
/// Formula: repay = borrow + ceil(borrow × 9 / 10_000)
pub fn solend_repay_amount(borrow_lamports: u64) -> u64 {
    let fee_numerator: u128 = borrow_lamports as u128 * 9;
    let fee = ((fee_numerator + 9_999) / 10_000) as u64; // ceiling division
    borrow_lamports + fee
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repay_includes_solend_fee() {
        // 1 SOL → fee = 900_000 lamports (0.09%)
        assert_eq!(solend_repay_amount(1_000_000_000), 1_000_900_000);
    }

    #[test]
    fn repay_rounds_up() {
        // 10_001 * 9 = 90_009 → ceil(90_009/10_000) = 10 lamports fee
        assert_eq!(solend_repay_amount(10_001), 10_011);
    }

    #[test]
    fn repay_minimum_is_one_lamport_fee() {
        // Even tiny borrows pay at least 1 lamport fee
        let repay = solend_repay_amount(1);
        assert!(repay > 1, "Even 1 lamport borrow should have a fee");
    }
}
