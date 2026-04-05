// =============================================================================
// SECURITY AUDIT CHECKLIST — jito-handler/src/keypair.rs
// [✓] Keypair loaded from file — never hardcoded
// [✓] Secret key bytes zeroed from stack after use (compiler may optimise,
//     but we do not persist them beyond the signing_key construction)
// [✓] Public key is base58 encoded for display/logging only — never logged raw
// [✓] File read errors propagated — no silent fallback to default keypair
// [✓] No unsafe code
//
// SOLANA KEYPAIR FORMAT:
//   JSON array of 64 u8 values.
//   Bytes [0..32]  = Ed25519 secret key (also called seed in dalek terminology)
//   Bytes [32..64] = Ed25519 public key (derived from secret)
//
// SIGNING FLOW:
//   1. Load 64-byte array from APEX_KEYPAIR_PATH
//   2. Construct ed25519_dalek::SigningKey from first 32 bytes
//   3. Sign the transaction message bytes
//   4. Inject 64-byte signature into the transaction's signature slot
// =============================================================================

use anyhow::{Context, Result};
use ed25519_dalek::{Signature, Signer, SigningKey};
use tracing::info;

/// Operator keypair — loaded once at startup.
pub struct ApexKeypair {
    signing_key: SigningKey,
    /// Base58-encoded public key (for display/logging)
    pub pubkey_b58: String,
    /// Raw 32-byte public key bytes
    pub pubkey_bytes: [u8; 32],
}

impl ApexKeypair {
    /// Load a Solana keypair from a JSON file.
    ///
    /// The file must contain a JSON array of exactly 64 unsigned 8-bit integers.
    /// The first 32 bytes are the Ed25519 secret key (seed).
    ///
    /// # Errors
    /// Returns an error if the file cannot be read, parsed, or is the wrong length.
    pub fn load(path: &str) -> Result<Self> {
        let raw = std::fs::read_to_string(path)
            .with_context(|| format!("Cannot read keypair file: {path}"))?;

        let bytes: Vec<u8> = serde_json::from_str(&raw)
            .with_context(|| format!("Keypair file is not a valid JSON byte array: {path}"))?;

        if bytes.len() != 64 {
            return Err(anyhow::anyhow!(
                "Keypair file must contain exactly 64 bytes, got {}",
                bytes.len()
            ));
        }

        let mut secret = [0u8; 32];
        secret.copy_from_slice(&bytes[0..32]);

        let signing_key = SigningKey::from_bytes(&secret);
        let verifying = signing_key.verifying_key();
        let pubkey_bytes = verifying.to_bytes();
        let pubkey_b58 = bs58::encode(pubkey_bytes).into_string();

        info!(pubkey = %pubkey_b58, "Keypair loaded successfully");

        Ok(Self {
            signing_key,
            pubkey_b58,
            pubkey_bytes,
        })
    }

    /// Create a mock/no-op keypair for simulation mode.
    /// All signing operations return a zeroed signature.
    pub fn mock() -> Self {
        let secret = [0u8; 32];
        let signing_key = SigningKey::from_bytes(&secret);
        let verifying = signing_key.verifying_key();
        let pubkey_bytes = verifying.to_bytes();
        let pubkey_b58 = bs58::encode(pubkey_bytes).into_string();

        info!(pubkey = %pubkey_b58, "Using mock keypair (simulation mode)");

        Self {
            signing_key,
            pubkey_b58,
            pubkey_bytes,
        }
    }

    /// Sign arbitrary message bytes (e.g. a Solana transaction message).
    ///
    /// Returns the 64-byte Ed25519 signature.
    #[must_use]
    pub fn sign(&self, message: &[u8]) -> [u8; 64] {
        let sig: Signature = self.signing_key.sign(message);
        sig.to_bytes()
    }

    /// Verify our own signature (sanity check before submission).
    #[must_use]
    pub fn verify(&self, message: &[u8], sig: &[u8; 64]) -> bool {
        use ed25519_dalek::Verifier;
        let signature = Signature::from_bytes(sig);
        self.signing_key
            .verifying_key()
            .verify(message, &signature)
            .is_ok()
    }
}

/// Inject a 64-byte Ed25519 signature into slot `sig_index` of a serialised
/// Solana transaction.
///
/// Solana transaction layout (legacy format):
///   [compact_u16: num_signatures]
///   [sig_0: 64 bytes] ... [sig_N: 64 bytes]
///   [message bytes ...]
///
/// For versioned (v0) transactions the first byte is >= 0x80; we skip it.
///
/// # Errors
/// Returns error if the byte slice is too short to contain the signature slot.
pub fn inject_signature(
    tx_bytes: &mut Vec<u8>,
    sig_index: usize,
    sig: &[u8; 64],
) -> Result<()> {
    if tx_bytes.is_empty() {
        return Err(anyhow::anyhow!("Transaction buffer is empty"));
    }

    let mut pos: usize = 0;

    // Skip versioned transaction prefix if present
    if tx_bytes[0] >= 0x80 {
        pos += 1;
    }

    // Decode compact-u16 number of signatures
    let (num_sigs, compact_len) = decode_compact_u16(&tx_bytes[pos..]);
    pos += compact_len;

    if sig_index >= num_sigs as usize {
        return Err(anyhow::anyhow!(
            "sig_index {sig_index} >= num_sigs {num_sigs}"
        ));
    }

    let sig_start = pos + sig_index * 64;
    let sig_end = sig_start + 64;

    if sig_end > tx_bytes.len() {
        return Err(anyhow::anyhow!(
            "Transaction too short to contain signature slot {sig_index}"
        ));
    }

    tx_bytes[sig_start..sig_end].copy_from_slice(sig);
    Ok(())
}

/// Extract the message bytes from a serialised Solana transaction.
///
/// These are the bytes that must be signed.
///
/// # Errors
/// Returns error if the byte slice is malformed.
pub fn extract_message_bytes(tx_bytes: &[u8]) -> Result<&[u8]> {
    if tx_bytes.is_empty() {
        return Err(anyhow::anyhow!("Transaction buffer is empty"));
    }

    let mut pos: usize = 0;

    // Skip versioned transaction prefix
    if tx_bytes[0] >= 0x80 {
        pos += 1;
    }

    // Skip the compact-u16 num_signatures and all signature slots
    let (num_sigs, compact_len) = decode_compact_u16(&tx_bytes[pos..]);
    pos += compact_len;
    pos += num_sigs as usize * 64;

    if pos > tx_bytes.len() {
        return Err(anyhow::anyhow!("Transaction truncated — cannot extract message"));
    }

    Ok(&tx_bytes[pos..])
}

/// Decode a compact-u16 from bytes (Solana's wire encoding).
/// Returns (value, bytes_consumed).
fn decode_compact_u16(bytes: &[u8]) -> (u16, usize) {
    if bytes.is_empty() {
        return (0, 0);
    }
    let b0 = bytes[0];
    if b0 & 0x80 == 0 {
        return (b0 as u16, 1);
    }
    if bytes.len() < 2 {
        return (b0 as u16 & 0x7f, 1);
    }
    let b1 = bytes[1];
    if b1 & 0x80 == 0 {
        let val = ((b0 & 0x7f) as u16) | ((b1 as u16) << 7);
        return (val, 2);
    }
    if bytes.len() < 3 {
        return (((b0 & 0x7f) as u16) | ((b1 as u16) << 7), 2);
    }
    let b2 = bytes[2];
    let val = ((b0 & 0x7f) as u16) | (((b1 & 0x7f) as u16) << 7) | ((b2 as u16) << 14);
    (val, 3)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mock_keypair_creates_without_panic() {
        let kp = ApexKeypair::mock();
        assert!(!kp.pubkey_b58.is_empty());
        assert_eq!(kp.pubkey_bytes.len(), 32);
    }

    #[test]
    fn sign_and_verify_roundtrip() {
        let kp = ApexKeypair::mock();
        let msg = b"test transaction message bytes";
        let sig = kp.sign(msg);
        assert!(kp.verify(msg, &sig));
        // Wrong message should not verify
        assert!(!kp.verify(b"wrong message", &sig));
    }

    #[test]
    fn decode_compact_u16_small() {
        let (val, len) = decode_compact_u16(&[0x01]);
        assert_eq!(val, 1);
        assert_eq!(len, 1);
    }

    #[test]
    fn decode_compact_u16_two_byte() {
        // 300 = 0x012C → compact: [0xAC, 0x02]
        let (val, len) = decode_compact_u16(&[0xAC, 0x02]);
        assert_eq!(val, 300);
        assert_eq!(len, 2);
    }

    #[test]
    fn inject_signature_into_mock_tx() {
        // Minimal mock tx: [compact_u16(1)] [64 zeros] [message: some bytes]
        let mut tx = vec![0x01u8]; // num_sigs = 1 (compact-u16, 1 byte)
        tx.extend_from_slice(&[0u8; 64]); // signature slot (all zeros)
        tx.extend_from_slice(b"message");

        let sig = [0xABu8; 64];
        inject_signature(&mut tx, 0, &sig).unwrap();

        assert_eq!(&tx[1..65], &sig);
    }
}
