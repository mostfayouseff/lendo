// crates/jito-handler/src/flash_tx.rs
// Clean Atomic Solend Flash Loan Builder (Final Fixed Version)

use crate::keypair::ApexKeypair;
use sha2::{Digest, Sha256};
use tracing::{info, warn};

// ── Mainnet Solend Addresses ────────────────────────────────────────────────
const SOLEND_PROGRAM_ID: &str = "So1endDq2YkqhipRh3WViPa8hdiSpxWy6z3Z6tMCpAo";
const SOLEND_MAIN_POOL: &str = "4UpD2fh7xH3VP9QQaXtsS1YY3bxzWhtfpks7FatyKvdY";
const SOLEND_SOL_RESERVE: &str = "FzbfXR7sopQL29Ubu312tkqWMxSre4dYSrFyYAjUYiC";
const SOLEND_SOL_LIQ_SUPPLY: &str = "8UviNr47S8eL6J3WfDxMRa3hvLta1VDJwNAqwTKZcZvj";
const SOLEND_FEE_RECEIVER: &str = "AXuN52TrDFhw9S8V3gfJvAX2JKK4y9ZtVGnE27PaLqQ";

const TOKEN_PROGRAM_ID: &str = "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA";
const WSOL_MINT: &str = "So11111111111111111111111111111111111111112";
const SYSVAR_INSTRUCTIONS: &str = "Sysvar1nstructions1111111111111111111111111";

const BORROW_DISCRIMINATOR: u8 = 0x14;
const REPAY_DISCRIMINATOR: u8 = 0x15;

/// Simple instruction for swaps (compatible with your old swap_data)
#[derive(Clone, Debug)]
pub struct TxInstruction {
    pub program_id: String,
    pub data: Vec<u8>,
}

/// Build atomic flash loan: Borrow → Swaps → Repay
pub fn build_flash_loan_tx(
    keypair: &ApexKeypair,
    blockhash_b58: &str,
    borrow_lamports: u64,
    repay_lamports: u64,
    swap_data: &[(String, Vec<u8>)],   // (program_id_b58, instruction_data)
) -> Vec<u8> {
    info!(
        borrow_sol = format!("{:.6}", borrow_lamports as f64 / 1e9),
        repay_sol = format!("{:.6}", repay_lamports as f64 / 1e9),
        swap_count = swap_data.len(),
        "FLASH LOAN TX: building atomic borrow → swaps → repay"
    );

    let operator = keypair.pubkey_bytes;
    let (lending_mkt_authority, _) = find_program_address(&[&b58_to_32(SOLEND_MAIN_POOL)], &b58_to_32(SOLEND_PROGRAM_ID));
    let (borrower_wsol_ata, _) = find_associated_token_account(&operator, &b58_to_32(WSOL_MINT));

    // Accounts table
    let mut accounts: Vec<[u8; 32]> = vec![
        operator,
        b58_to_32(SOLEND_SOL_LIQ_SUPPLY),
        borrower_wsol_ata,
        b58_to_32(SOLEND_SOL_RESERVE),
        b58_to_32(SOLEND_FEE_RECEIVER),
        lending_mkt_authority,
    ];

    let solend_idx = accounts.len() as u8;
    accounts.push(b58_to_32(SOLEND_PROGRAM_ID));

    let token_idx = accounts.len() as u8;
    accounts.push(b58_to_32(TOKEN_PROGRAM_ID));

    let sysvar_idx = accounts.len() as u8;
    accounts.push(b58_to_32(SYSVAR_INSTRUCTIONS));

    let lending_market_idx = accounts.len() as u8;
    accounts.push(b58_to_32(SOLEND_MAIN_POOL));

    // Add DEX programs
    let mut dex_indices = std::collections::HashMap::new();
    for (prog_id, _) in swap_data {
        if !dex_indices.contains_key(prog_id) {
            let idx = accounts.len() as u8;
            dex_indices.insert(prog_id.clone(), idx);
            accounts.push(b58_to_32(prog_id));
        }
    }

    // Build instructions
    let mut instructions = Vec::new();

    // Flash Borrow
    let mut borrow_data = vec![BORROW_DISCRIMINATOR];
    borrow_data.extend_from_slice(&borrow_lamports.to_le_bytes());
    instructions.push((solend_idx, vec![1, 2, 3, 4, 4, lending_market_idx, 5, token_idx, sysvar_idx], borrow_data));

    // Swaps
    for (prog_id, data) in swap_data {
        let prog_idx = *dex_indices.get(prog_id).unwrap_or(&solend_idx);
        instructions.push((prog_idx, vec![0, 2], data.clone())); // operator + wsol_ata (simplified)
    }

    // Flash Repay
    let mut repay_data = vec![REPAY_DISCRIMINATOR];
    repay_data.extend_from_slice(&repay_lamports.to_le_bytes());
    repay_data.push(0); // borrow ix index
    instructions.push((solend_idx, vec![2, 1, 3, 4, 4, lending_market_idx, 5, 0, token_idx, sysvar_idx], repay_data));

    // Serialize legacy transaction
    let mut msg: Vec<u8> = Vec::new();
    msg.push(1); // 1 signer
    msg.push(0); // readonly signed
    msg.push((accounts.len() as u8).saturating_sub(1)); // readonly unsigned

    write_compact_u16(&mut msg, accounts.len() as u16);
    for acct in &accounts {
        msg.extend_from_slice(acct);
    }

    msg.extend_from_slice(&b58_to_32(blockhash_b58));

    write_compact_u16(&mut msg, instructions.len() as u16);
    for (prog_idx, acct_idxs, data) in &instructions {
        msg.push(*prog_idx);
        write_compact_u16(&mut msg, acct_idxs.len() as u16);
        for &idx in acct_idxs {
            msg.push(idx);
        }
        write_compact_u16(&mut msg, data.len() as u16);
        msg.extend_from_slice(data);
    }

    let sig = keypair.sign(&msg);
    let mut tx = Vec::new();
    tx.push(1u8);
    tx.extend_from_slice(&sig);
    tx.extend_from_slice(&msg);

    info!(tx_size = tx.len(), "FLASH LOAN TX: complete atomic transaction built and signed");
    tx
}

// ── Utilities ───────────────────────────────────────────────────────────────
pub fn write_compact_u16(buf: &mut Vec<u8>, mut val: u16) {
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

pub fn b58_to_32(addr: &str) -> [u8; 32] {
    let decoded = bs58::decode(addr).into_vec().unwrap_or_default();
    if decoded.len() != 32 {
        warn!(addr = %addr, "b58_to_32 failed");
        return [0u8; 32];
    }
    let mut out = [0u8; 32];
    out.copy_from_slice(&decoded);
    out
}

pub fn find_program_address(seeds: &[&[u8]], program_id: &[u8; 32]) -> ([u8; 32], u8) {
    for nonce in (0u8..=255).rev() {
        let mut hasher = Sha256::new();
        for seed in seeds {
            hasher.update(seed);
        }
        hasher.update([nonce]);
        hasher.update(program_id);
        hasher.update(b"ProgramDerivedAddress");
        let hash: [u8; 32] = hasher.finalize().into();
        if !is_on_curve(&hash) {
            return (hash, nonce);
        }
    }
    ([0u8; 32], 0)
}

pub fn find_associated_token_account(owner: &[u8; 32], mint: &[u8; 32]) -> ([u8; 32], u8) {
    let token_prog = b58_to_32(TOKEN_PROGRAM_ID);
    let ata_prog = b58_to_32(ATA_PROGRAM_ID); // you can add this const if needed
    find_program_address(&[owner, &token_prog, mint], &ata_prog)
}

fn is_on_curve(bytes: &[u8; 32]) -> bool {
    use curve25519_dalek::edwards::CompressedEdwardsY;
    CompressedEdwardsY(*bytes).decompress().is_some()
}
