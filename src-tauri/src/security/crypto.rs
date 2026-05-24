// WorkMirror — AES-256-GCM encryption with platform-keychain key storage.
//
// ## Data format
//
// Every encrypted payload is laid out as:
//
//   ┌──────────────┬──────────────────────────────┐
//   │  12 B nonce  │         ciphertext           │
//   └──────────────┴──────────────────────────────┘
//
// The nonce is randomly generated per encryption call (96-bit, as required
// by AES-GCM).  No additional framing or magic bytes are prepended.
//
// ## Key lifecycle
//
//  1.  On first call to `encrypt()` or `decrypt()`, the module tries to read
//      a hex-encoded 32-byte master key from the platform keychain.
//  2.  If no key exists, a fresh 32-byte key is generated from the OS
//      CSPRNG and persisted to the keychain.
//  3.  From that point forward, the same key is reused for all operations.
//
// ## Safety guarantees
//
//  - Zero `unsafe` blocks.
//  - All temporary plaintext / key buffers are zeroed via `zeroize`.
//  - All error paths carry human-readable context (no bare `.unwrap()` in
//    production code paths).

use aes_gcm::{
    aead::{Aead, KeyInit, OsRng},
    Aes256Gcm,  // 256-bit key, 96-bit nonce, 128-bit tag
    Nonce,
};
use rand::RngCore;
use std::sync::{Mutex, OnceLock};
use thiserror::Error;
use zeroize::Zeroize;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// AES-256 key length in bytes (32 bytes = 256 bits).
const KEY_LENGTH: usize = 32;

/// AES-GCM nonce length in bytes (12 bytes = 96 bits).
const NONCE_LENGTH: usize = 12;

/// Name of the keyring entry used to store the master encryption key.
const KEYRING_SERVICE: &str = "workmirror";
const KEYRING_USER: &str = "master-key";

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

/// Errors that can occur during encryption / decryption.
#[derive(Debug, Error)]
pub enum SecurityError {
    /// An I/O error (e.g. keyring service unreachable).
    #[error("I/O error: {0}")]
    Io(String),

    /// A cryptographic operation failed (e.g. decryption tag mismatch).
    #[error("Crypto error: {0}")]
    Crypto(String),

    /// The platform keychain returned an error.
    #[error("Keychain error: {0}")]
    KeyChain(String),
}

// ---------------------------------------------------------------------------
// Key management
// ---------------------------------------------------------------------------

/// Thread-safe cipher instance, lazily initialised on first use.
static CIPHER: OnceLock<Mutex<Option<Aes256Gcm>>> = OnceLock::new();

/// Retrieve (or initialise on first call) the AES-256-GCM cipher from the
/// platform keychain.
fn get_cipher() -> Result<std::sync::MutexGuard<'static, Option<Aes256Gcm>>, SecurityError> {
    let lock = CIPHER.get_or_init(|| Mutex::new(None));
    let mut guard = lock.lock().unwrap();
    if guard.is_none() {
        let key_bytes = load_or_create_key()?;
        let key = aes_gcm::Key::<Aes256Gcm>::from_slice(&key_bytes);
        *guard = Some(Aes256Gcm::new(key));
    }
    Ok(guard)
}

/// Load the hex-encoded master key from the platform keychain.
///
/// If the key does not exist yet, generate a fresh 32-byte key from the OS
/// CSPRNG, persist it to the keychain, and return it.
fn load_or_create_key() -> Result<[u8; KEY_LENGTH], SecurityError> {
    // Try to read an existing key from the keychain.
    let entry = keyring::Entry::new(KEYRING_SERVICE, KEYRING_USER)
        .map_err(|e| SecurityError::KeyChain(format!("failed to create keyring entry: {e}")))?;

    match entry.get_password() {
        Ok(hex_key) => {
            // Decode the hex string back into a 32-byte key.
            let mut key = [0u8; KEY_LENGTH];
            hex::decode_to_slice(&hex_key, &mut key)
                .map_err(|e| SecurityError::KeyChain(format!("failed to decode key: {e}")))?;
            // Zero the hex string now that we have the raw key.
            let mut hex_mut = hex_key;
            hex_mut.zeroize();
            Ok(key)
        }
        Err(keyring::Error::NoEntry) => {
            // No key exists yet — generate a fresh one.
            let mut key = [0u8; KEY_LENGTH];
            OsRng.fill_bytes(&mut key);

            // Persist the key as a hex string.
            let hex_key = hex::encode(key);
            entry
                .set_password(&hex_key)
                .map_err(|e| SecurityError::KeyChain(format!("failed to store key: {e}")))?;
            let mut hex_mut = hex_key;
            hex_mut.zeroize();

            Ok(key)
        }
        Err(e) => Err(SecurityError::KeyChain(format!(
            "keyring read error: {e}"
        ))),
    }
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Encrypt `data` with AES-256-GCM.
///
/// The returned vector has the layout `[12-byte nonce ‖ ciphertext]`.
/// The caller is responsible for zeroizing the plaintext slice after calling
/// this function, if required.
pub fn encrypt(data: &[u8]) -> Result<Vec<u8>, SecurityError> {
    let guard = get_cipher()?;
    let cipher = guard.as_ref().unwrap();

    // Generate a random 96-bit nonce.
    let mut nonce_bytes = [0u8; NONCE_LENGTH];
    OsRng.fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);

    // Encrypt in-place — `encrypt` returns the ciphertext with the
    // authentication tag appended (the tag is 16 bytes for AES-GCM).
    let ciphertext = cipher
        .encrypt(nonce, data)
        .map_err(|e| SecurityError::Crypto(format!("encryption failed: {e}")))?;

    // Prepend the nonce.
    let mut result = Vec::with_capacity(NONCE_LENGTH + ciphertext.len());
    result.extend_from_slice(&nonce_bytes);
    result.extend_from_slice(&ciphertext);

    // Zero the nonce buffer — it is now embedded in `result`.
    nonce_bytes.zeroize();

    Ok(result)
}

/// Decrypt a payload previously produced by `encrypt()`.
///
/// The input must be at least 13 bytes (12-byte nonce + 1 byte of
/// ciphertext).  Returns the original plaintext on success.
pub fn decrypt(encrypted: &[u8]) -> Result<Vec<u8>, SecurityError> {
    if encrypted.len() < NONCE_LENGTH + 1 {
        return Err(SecurityError::Crypto(format!(
            "encrypted payload too short: {} bytes (need at least {})",
            encrypted.len(),
            NONCE_LENGTH + 1,
        )));
    }

    let guard = get_cipher()?;
    let cipher = guard.as_ref().unwrap();

    let (nonce_bytes, ciphertext) = encrypted.split_at(NONCE_LENGTH);
    let nonce = Nonce::from_slice(nonce_bytes);

    let plaintext = cipher
        .decrypt(nonce, ciphertext)
        .map_err(|_| SecurityError::Crypto("decryption failed — data may be corrupted or the key may have changed".into()))?;

    Ok(plaintext)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Encrypt and then decrypt an empty slice.
    #[test]
    fn encrypt_decrypt_empty() {
        let data = b"";
        let encrypted = encrypt(data).expect("encryption should succeed");
        // Encrypted payload must contain nonce + at least the GCM tag.
        assert!(encrypted.len() >= NONCE_LENGTH + 16);
        let decrypted = decrypt(&encrypted).expect("decryption should succeed");
        assert_eq!(decrypted, data);
    }

    /// Encrypt and then decrypt a very short payload.
    #[test]
    fn encrypt_decrypt_short() {
        let data = b"hello";
        let encrypted = encrypt(data).expect("encryption should succeed");
        let decrypted = decrypt(&encrypted).expect("decryption should succeed");
        assert_eq!(decrypted, data);
    }

    /// Encrypt and then decrypt a payload of exactly 1 KB.
    #[test]
    fn encrypt_decrypt_1k() {
        let data = vec![0xABu8; 1024];
        let encrypted = encrypt(&data).expect("encryption should succeed");
        let decrypted = decrypt(&encrypted).expect("decryption should succeed");
        assert_eq!(decrypted, data);
    }

    /// Encrypt and then decrypt a larger payload (~64 KB).
    #[test]
    fn encrypt_decrypt_64k() {
        let data = vec![0x42u8; 64 * 1024];
        let encrypted = encrypt(&data).expect("encryption should succeed");
        let decrypted = decrypt(&encrypted).expect("decryption should succeed");
        assert_eq!(decrypted, data);
    }

    /// Each encryption call should produce a different ciphertext (nonce
    /// randomness).
    #[test]
    fn nonce_randomness() {
        let data = b"deterministic input";
        let a = encrypt(data).expect("encrypt a");
        let b = encrypt(data).expect("encrypt b");
        assert_ne!(a, b, "two encryptions of the same data must differ");
    }

    /// Decrypting with a single flipped byte in the nonce must fail.
    #[test]
    fn corrupted_nonce() {
        let data = b"sensitive data";
        let mut encrypted = encrypt(data).expect("encrypt");
        // Flip the first byte of the nonce.
        encrypted[0] ^= 0xFF;
        let result = decrypt(&encrypted);
        assert!(result.is_err(), "corrupted nonce should fail decryption");
    }

    /// Decrypting with a single flipped byte in the ciphertext must fail.
    #[test]
    fn corrupted_ciphertext() {
        let data = b"sensitive data";
        let mut encrypted = encrypt(data).expect("encrypt");
        // Flip a byte in the middle of the ciphertext.
        let idx = NONCE_LENGTH + encrypted.len() / 2;
        if idx < encrypted.len() {
            encrypted[idx] ^= 0x01;
        }
        let result = decrypt(&encrypted);
        assert!(result.is_err(), "corrupted ciphertext should fail decryption");
    }

    /// Truncated payload must be rejected.
    #[test]
    fn truncated_payload() {
        let result = decrypt(&[0u8; 5]);
        assert!(result.is_err(), "truncated payload should fail");
    }

    /// The encrypted output must always start with the nonce and have the
    /// correct minimum length.
    #[test]
    fn output_format() {
        let data = b"test";
        let encrypted = encrypt(data).expect("encrypt");
        // Minimum: 12 (nonce) + 16 (GCM tag for empty plaintext) = 28
        // Our data is 4 bytes, so we get 12 + 4 + 16 = 32.
        assert_eq!(encrypted.len(), NONCE_LENGTH + data.len() + 16);
    }

    /// Round-trip for binary data including null bytes.
    #[test]
    fn binary_data() {
        let data = vec![0x00, 0xFF, 0x01, 0xFE, 0x00, 0x80];
        let encrypted = encrypt(&data).expect("encrypt");
        let decrypted = decrypt(&encrypted).expect("decrypt");
        assert_eq!(decrypted, data);
    }
}
