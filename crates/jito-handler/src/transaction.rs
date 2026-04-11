use crate::keypair::ApexKeypair;
use anyhow::{anyhow, Result};
use base64::Engine as _;

const SYSTEM_PROGRAM: &str = "11111111111111111111111111111111";

pub struct SignedTipTransaction {
    pub tx_bytes: Vec<u8>,
    pub signature: String,
}

pub fn attach_jito_tip_and_sign(
    unsigned_tx_b64: &str,
    keypair: &ApexKeypair,
    tip_account_b58: &str,
    tip_lamports: u64,
) -> Result<SignedTipTransaction> {
    if tip_lamports == 0 {
        return Err(anyhow!("Jito tip lamports must be > 0"));
    }

    let mut tx = base64::engine::general_purpose::STANDARD
        .decode(unsigned_tx_b64)
        .map_err(|e| anyhow!("Ultra transaction base64 decode failed: {e}"))?;

    let tip_account = pubkey_bytes(tip_account_b58)?;
    let system_program = pubkey_bytes(SYSTEM_PROGRAM)?;

    let mut pos = 0usize;
    let (sig_count, sig_len) = read_shortvec(&tx, pos)?;
    pos += sig_len;
    if sig_count != 1 {
        return Err(anyhow!(
            "Ultra transaction requires {sig_count} signatures; this bot can safely sign exactly 1"
        ));
    }
    let sig_start = pos;
    let sig_end = sig_start + 64 * sig_count;
    ensure_len(&tx, sig_end, "signature section")?;

    let msg_start = sig_end;
    ensure_len(&tx, msg_start + 1, "message prefix")?;
    let versioned = tx[msg_start] & 0x80 != 0;
    if versioned && tx[msg_start] != 0x80 {
        return Err(anyhow!("Unsupported Solana transaction version byte: {}", tx[msg_start]));
    }

    let header_start = if versioned { msg_start + 1 } else { msg_start };
    ensure_len(&tx, header_start + 3, "message header")?;
    let num_required_signatures = tx[header_start] as usize;
    let num_readonly_unsigned = tx[header_start + 2] as usize;
    if num_required_signatures != 1 {
        return Err(anyhow!(
            "Ultra message requires {num_required_signatures} signers; expected exactly the operator signer"
        ));
    }

    let keys_len_start = header_start + 3;
    let (key_count, key_count_len) = read_shortvec(&tx, keys_len_start)?;
    let keys_start = keys_len_start + key_count_len;
    let keys_end = keys_start + key_count * 32;
    ensure_len(&tx, keys_end + 32, "account keys and recent blockhash")?;

    if tx[keys_start..keys_start + 32] != keypair.pubkey_bytes {
        return Err(anyhow!(
            "Ultra transaction fee payer does not match loaded operator wallet"
        ));
    }

    let mut keys = Vec::with_capacity(key_count + 2);
    for i in 0..key_count {
        let mut key = [0u8; 32];
        key.copy_from_slice(&tx[keys_start + i * 32..keys_start + (i + 1) * 32]);
        keys.push(key);
    }

    let old_static_len = keys.len();
    if num_readonly_unsigned > old_static_len.saturating_sub(num_required_signatures) {
        return Err(anyhow!("Malformed message header: readonly unsigned count exceeds unsigned accounts"));
    }

    let insert_pos = old_static_len - num_readonly_unsigned;
    keys.insert(insert_pos, tip_account);
    let tip_index = insert_pos;
    let mut readonly_unsigned_delta = 0usize;

    let system_index = match keys.iter().position(|k| *k == system_program) {
        Some(idx) => idx,
        None => {
            keys.push(system_program);
            readonly_unsigned_delta = 1;
            keys.len() - 1
        }
    };

    if tip_index > u8::MAX as usize || system_index > u8::MAX as usize {
        return Err(anyhow!("Cannot append Jito tip: account index exceeds u8 compiled instruction limit"));
    }

    tx[header_start + 2] = (num_readonly_unsigned + readonly_unsigned_delta)
        .try_into()
        .map_err(|_| anyhow!("readonly unsigned account count overflow"))?;

    let blockhash_start = keys_end;
    let blockhash_end = blockhash_start + 32;
    let ix_count_start = blockhash_end;
    let (ix_count, ix_count_len) = read_shortvec(&tx, ix_count_start)?;
    let mut cursor = ix_count_start + ix_count_len;

    let mut adjusted_ixs = Vec::new();
    for _ in 0..ix_count {
        ensure_len(&tx, cursor + 1, "compiled instruction program id")?;
        let mut program_id_index = tx[cursor];
        cursor += 1;
        adjust_index(&mut program_id_index, insert_pos)?;

        let (acct_count, acct_count_len) = read_shortvec(&tx, cursor)?;
        cursor += acct_count_len;
        ensure_len(&tx, cursor + acct_count, "compiled instruction accounts")?;
        let mut accounts = tx[cursor..cursor + acct_count].to_vec();
        for acct in &mut accounts {
            adjust_index(acct, insert_pos)?;
        }
        cursor += acct_count;

        let (data_len, data_len_size) = read_shortvec(&tx, cursor)?;
        cursor += data_len_size;
        ensure_len(&tx, cursor + data_len, "compiled instruction data")?;
        let data = tx[cursor..cursor + data_len].to_vec();
        cursor += data_len;

        adjusted_ixs.push(program_id_index);
        write_shortvec(accounts.len(), &mut adjusted_ixs);
        adjusted_ixs.extend_from_slice(&accounts);
        write_shortvec(data.len(), &mut adjusted_ixs);
        adjusted_ixs.extend_from_slice(&data);
    }

    let lookup_bytes = if versioned {
        tx[cursor..].to_vec()
    } else {
        Vec::new()
    };
    if !versioned && cursor != tx.len() {
        return Err(anyhow!("Malformed legacy transaction: trailing bytes after instructions"));
    }

    let mut tip_data = vec![2u8, 0, 0, 0];
    tip_data.extend_from_slice(&tip_lamports.to_le_bytes());
    adjusted_ixs.push(system_index as u8);
    write_shortvec(2, &mut adjusted_ixs);
    adjusted_ixs.push(0u8);
    adjusted_ixs.push(tip_index as u8);
    write_shortvec(tip_data.len(), &mut adjusted_ixs);
    adjusted_ixs.extend_from_slice(&tip_data);

    let mut rebuilt = Vec::with_capacity(tx.len() + 80);
    rebuilt.extend_from_slice(&tx[..msg_start]);
    if versioned {
        rebuilt.push(0x80);
    }
    rebuilt.extend_from_slice(&tx[header_start..header_start + 3]);
    write_shortvec(keys.len(), &mut rebuilt);
    for key in &keys {
        rebuilt.extend_from_slice(key);
    }
    rebuilt.extend_from_slice(&tx[blockhash_start..blockhash_end]);
    write_shortvec(ix_count + 1, &mut rebuilt);
    rebuilt.extend_from_slice(&adjusted_ixs);
    rebuilt.extend_from_slice(&lookup_bytes);

    let message = &rebuilt[msg_start..];
    let signature = keypair.sign(message);
    if !keypair.verify(message, &signature) {
        return Err(anyhow!("Operator signature self-verification failed"));
    }
    rebuilt[sig_start..sig_start + 64].copy_from_slice(&signature);

    Ok(SignedTipTransaction {
        tx_bytes: rebuilt,
        signature: bs58::encode(signature).into_string(),
    })
}

fn adjust_index(index: &mut u8, insert_pos: usize) -> Result<()> {
    if (*index as usize) >= insert_pos {
        *index = index
            .checked_add(1)
            .ok_or_else(|| anyhow!("compiled instruction account index overflow"))?;
    }
    Ok(())
}

fn pubkey_bytes(value: &str) -> Result<[u8; 32]> {
    let decoded = bs58::decode(value)
        .into_vec()
        .map_err(|e| anyhow!("invalid base58 pubkey {value}: {e}"))?;
    if decoded.len() != 32 {
        return Err(anyhow!("pubkey {value} decoded to {} bytes, expected 32", decoded.len()));
    }
    let mut out = [0u8; 32];
    out.copy_from_slice(&decoded);
    Ok(out)
}

fn ensure_len(bytes: &[u8], needed: usize, label: &str) -> Result<()> {
    if bytes.len() < needed {
        return Err(anyhow!("transaction truncated while reading {label}"));
    }
    Ok(())
}

fn read_shortvec(bytes: &[u8], start: usize) -> Result<(usize, usize)> {
    let mut value = 0usize;
    let mut shift = 0usize;
    for i in 0..3 {
        let b = *bytes
            .get(start + i)
            .ok_or_else(|| anyhow!("shortvec truncated"))?;
        value |= ((b & 0x7f) as usize) << shift;
        if b & 0x80 == 0 {
            return Ok((value, i + 1));
        }
        shift += 7;
    }
    Err(anyhow!("shortvec exceeds supported u16 length"))
}

fn write_shortvec(mut value: usize, out: &mut Vec<u8>) {
    loop {
        let mut elem = (value & 0x7f) as u8;
        value >>= 7;
        if value == 0 {
            out.push(elem);
            break;
        }
        elem |= 0x80;
        out.push(elem);
    }
}
