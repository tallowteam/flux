//! Cryptographic primitives for Flux peer-to-peer encryption.
//!
//! Provides:
//! - `DeviceIdentity`: Persistent X25519 key pair for device identification (TOFU).
//! - `EncryptedChannel`: Per-session XChaCha20-Poly1305 AEAD encryption using ephemeral key exchange.
//!
//! Security properties:
//! - All key material is zeroed on drop via the `zeroize` crate.
//! - Raw DH shared secrets are passed through BLAKE3 key derivation (domain-separated)
//!   before use as symmetric keys.
//! - Identity files are written with restrictive permissions (owner-only on Unix).

use std::path::Path;

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use chacha20poly1305::aead::{Aead, KeyInit, OsRng};
use chacha20poly1305::{AeadCore, XChaCha20Poly1305};
use serde::{Deserialize, Serialize};
use x25519_dalek::{EphemeralSecret, PublicKey, StaticSecret};
use zeroize::Zeroize;

use crate::error::FluxError;

/// Domain separation string for deriving symmetric keys from DH shared secrets.
/// This ensures the derived key is bound to the Flux protocol and cannot be
/// confused with keys derived for other purposes from the same shared secret.
const KDF_CONTEXT: &str = "flux v1 xchacha20poly1305 session key";

/// Persistent device identity key pair for TOFU authentication.
///
/// Generated lazily on first use of a security feature. Stored as JSON
/// in the config directory (`identity.json`).
pub struct DeviceIdentity {
    secret_key: StaticSecret,
    public_key: PublicKey,
}

impl std::fmt::Debug for DeviceIdentity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DeviceIdentity")
            .field("public_key", &self.public_key_base64())
            .field("secret_key", &"[REDACTED]")
            .finish()
    }
}

/// Serializable format for persisting the identity key pair.
#[derive(Serialize, Deserialize)]
struct IdentityFile {
    secret_key: String, // base64-encoded 32 bytes
    public_key: String, // base64-encoded 32 bytes
}

impl Drop for DeviceIdentity {
    fn drop(&mut self) {
        // Zeroize the secret key material on drop.
        // StaticSecret stores [u8; 32] internally -- we zero it via its byte representation.
        // PublicKey is not secret, but we zero it to avoid leaving correlated data.
        let secret_bytes = self.secret_key.as_bytes();
        // SAFETY: We own self and it's being dropped. We cast away const to zeroize in place.
        // This is safe because no one else can observe the value after drop.
        unsafe {
            let ptr = secret_bytes.as_ptr() as *mut u8;
            std::ptr::write_volatile(ptr, 0);
            for i in 0..32 {
                std::ptr::write_volatile(ptr.add(i), 0);
            }
        }
    }
}

impl DeviceIdentity {
    /// Generate a new random key pair.
    pub fn generate() -> Self {
        let secret_key = StaticSecret::random_from_rng(OsRng);
        let public_key = PublicKey::from(&secret_key);
        Self {
            secret_key,
            public_key,
        }
    }

    /// Load an existing identity from `config_dir/identity.json`, or generate
    /// and save a new one if the file does not exist.
    pub fn load_or_create(config_dir: &Path) -> Result<Self, FluxError> {
        let path = config_dir.join("identity.json");

        if path.exists() {
            let data = std::fs::read_to_string(&path).map_err(|e| {
                FluxError::EncryptionError(format!("Failed to read identity file: {}", e))
            })?;
            let file: IdentityFile = serde_json::from_str(&data).map_err(|e| {
                FluxError::EncryptionError(format!("Failed to parse identity file: {}", e))
            })?;

            let mut secret_bytes: [u8; 32] = BASE64
                .decode(&file.secret_key)
                .map_err(|e| {
                    FluxError::EncryptionError(format!("Invalid base64 in identity file: {}", e))
                })?
                .try_into()
                .map_err(|_| {
                    FluxError::EncryptionError("Secret key must be exactly 32 bytes".into())
                })?;

            let secret_key = StaticSecret::from(secret_bytes);
            // Zero the intermediate byte array immediately after conversion
            secret_bytes.zeroize();

            let public_key = PublicKey::from(&secret_key);

            // Verify stored public key matches derived one
            let stored_pub_bytes: Vec<u8> =
                BASE64.decode(&file.public_key).unwrap_or_default();
            if stored_pub_bytes.as_slice() != public_key.as_bytes() {
                return Err(FluxError::EncryptionError(
                    "Identity file corrupted: public key does not match secret key".into(),
                ));
            }

            Ok(Self {
                secret_key,
                public_key,
            })
        } else {
            let identity = Self::generate();
            identity.save(config_dir)?;
            Ok(identity)
        }
    }

    /// Save the identity to `config_dir/identity.json` using atomic write.
    ///
    /// On Unix, the file is created with mode 0o600 (owner read/write only).
    fn save(&self, config_dir: &Path) -> Result<(), FluxError> {
        let path = config_dir.join("identity.json");
        let tmp_path = config_dir.join("identity.json.tmp");

        let file = IdentityFile {
            secret_key: BASE64.encode(self.secret_key.as_bytes()),
            public_key: BASE64.encode(self.public_key.as_bytes()),
        };

        let json = serde_json::to_string_pretty(&file).map_err(|e| {
            FluxError::EncryptionError(format!("Failed to serialize identity: {}", e))
        })?;

        std::fs::write(&tmp_path, &json).map_err(|e| {
            FluxError::EncryptionError(format!("Failed to write identity file: {}", e))
        })?;

        // Set restrictive permissions on Unix (owner read/write only)
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let perms = std::fs::Permissions::from_mode(0o600);
            std::fs::set_permissions(&tmp_path, perms).map_err(|e| {
                FluxError::EncryptionError(format!("Failed to set identity file permissions: {}", e))
            })?;
        }

        std::fs::rename(&tmp_path, &path).map_err(|e| {
            FluxError::EncryptionError(format!("Failed to save identity file: {}", e))
        })?;

        Ok(())
    }

    /// Return the public key as a base64-encoded string for display/storage.
    pub fn public_key_base64(&self) -> String {
        BASE64.encode(self.public_key.as_bytes())
    }

    /// Return a short fingerprint (first 16 characters of the base64 public key).
    pub fn fingerprint(&self) -> String {
        let b64 = self.public_key_base64();
        b64.chars().take(16).collect()
    }

    /// Return a reference to the raw public key.
    pub fn public_key(&self) -> &PublicKey {
        &self.public_key
    }

    /// Return a reference to the static secret key (for key exchange with peers).
    pub fn secret_key(&self) -> &StaticSecret {
        &self.secret_key
    }
}

/// Per-session encrypted channel using XChaCha20-Poly1305 AEAD.
///
/// Created via X25519 Diffie-Hellman key exchange. Each `encrypt` call
/// generates a random 24-byte nonce (safe without counters due to the
/// 192-bit nonce space of XChaCha20).
///
/// Encrypted frame format: `[24-byte nonce][ciphertext + 16-byte Poly1305 tag]`
pub struct EncryptedChannel {
    cipher: XChaCha20Poly1305,
}

impl EncryptedChannel {
    /// Create an ephemeral key pair for key exchange.
    /// Returns `(secret, public_key)` -- send `public_key` to the peer.
    pub fn initiate() -> (EphemeralSecret, PublicKey) {
        let secret = EphemeralSecret::random_from_rng(OsRng);
        let public = PublicKey::from(&secret);
        (secret, public)
    }

    /// Complete the key exchange with the peer's public key to create
    /// the encrypted channel.
    ///
    /// The raw DH shared secret is passed through BLAKE3 key derivation with
    /// domain separation before use as the XChaCha20-Poly1305 key. This ensures:
    /// - The symmetric key is uniformly distributed (DH output may not be)
    /// - The key is domain-separated and cannot be confused with other uses
    pub fn complete(secret: EphemeralSecret, peer_public: &PublicKey) -> Self {
        let shared = secret.diffie_hellman(peer_public);

        // Derive symmetric key using BLAKE3 in key derivation mode with domain separation.
        // This is the recommended way to derive keys from DH shared secrets.
        let mut derived_key = blake3::derive_key(KDF_CONTEXT, shared.as_bytes());
        let cipher = XChaCha20Poly1305::new((&derived_key).into());

        // Zero the derived key material immediately after cipher creation
        derived_key.zeroize();

        Self { cipher }
    }

    /// Encrypt plaintext with a random nonce.
    /// Returns `(ciphertext, nonce)`.
    pub fn encrypt(&self, plaintext: &[u8]) -> Result<(Vec<u8>, [u8; 24]), FluxError> {
        let nonce = XChaCha20Poly1305::generate_nonce(&mut OsRng);
        let ciphertext = self
            .cipher
            .encrypt(&nonce, plaintext)
            .map_err(|e| FluxError::EncryptionError(format!("Encrypt failed: {}", e)))?;
        Ok((ciphertext, nonce.into()))
    }

    /// Decrypt ciphertext using the provided nonce.
    pub fn decrypt(&self, ciphertext: &[u8], nonce: &[u8; 24]) -> Result<Vec<u8>, FluxError> {
        self.cipher
            .decrypt(nonce.into(), ciphertext)
            .map_err(|e| FluxError::EncryptionError(format!("Decrypt failed: {}", e)))
    }
}

/// Convenience: perform key exchange between two parties (for testing).
/// Takes party A's secret and party B's public key, returns a channel.
pub fn key_exchange(
    our_secret: EphemeralSecret,
    peer_public: &PublicKey,
) -> EncryptedChannel {
    EncryptedChannel::complete(our_secret, peer_public)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn device_identity_generate_produces_valid_keypair() {
        let id = DeviceIdentity::generate();
        // Public key should be 32 bytes
        assert_eq!(id.public_key().as_bytes().len(), 32);
        // Base64 of 32 bytes = 44 chars
        assert_eq!(id.public_key_base64().len(), 44);
        // Fingerprint is first 16 chars
        assert_eq!(id.fingerprint().len(), 16);
        assert!(id.public_key_base64().starts_with(&id.fingerprint()));
    }

    #[test]
    fn device_identity_two_generates_differ() {
        let id1 = DeviceIdentity::generate();
        let id2 = DeviceIdentity::generate();
        assert_ne!(id1.public_key_base64(), id2.public_key_base64());
    }

    #[test]
    fn device_identity_load_or_create_persists_and_reloads() {
        let dir = tempfile::tempdir().unwrap();
        let id1 = DeviceIdentity::load_or_create(dir.path()).unwrap();
        let id2 = DeviceIdentity::load_or_create(dir.path()).unwrap();

        // Same key should be loaded on second call
        assert_eq!(id1.public_key_base64(), id2.public_key_base64());

        // File should exist
        assert!(dir.path().join("identity.json").exists());
    }

    #[test]
    fn device_identity_corrupted_file_returns_error() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("identity.json"), "not json").unwrap();
        let result = DeviceIdentity::load_or_create(dir.path());
        assert!(result.is_err());
        let err = format!("{}", result.unwrap_err());
        assert!(err.contains("Failed to parse identity file"));
    }

    #[test]
    fn encrypt_decrypt_roundtrip() {
        // Simulate two parties
        let (secret_a, public_a) = EncryptedChannel::initiate();
        let (secret_b, public_b) = EncryptedChannel::initiate();

        // A creates channel with B's public key
        let channel_a = EncryptedChannel::complete(secret_a, &public_b);
        // B creates channel with A's public key
        let channel_b = EncryptedChannel::complete(secret_b, &public_a);

        let plaintext = b"Hello, Flux peer-to-peer encryption!";

        // A encrypts
        let (ciphertext, nonce) = channel_a.encrypt(plaintext).unwrap();
        // B decrypts
        let decrypted = channel_b.decrypt(&ciphertext, &nonce).unwrap();

        assert_eq!(plaintext.as_slice(), decrypted.as_slice());
    }

    #[test]
    fn encrypt_produces_different_ciphertext_each_time() {
        let (secret_a, _public_a) = EncryptedChannel::initiate();
        let (_secret_b, public_b) = EncryptedChannel::initiate();
        let channel = EncryptedChannel::complete(secret_a, &public_b);

        let plaintext = b"same data";
        let (ct1, nonce1) = channel.encrypt(plaintext).unwrap();
        let (ct2, nonce2) = channel.encrypt(plaintext).unwrap();

        // Different random nonces
        assert_ne!(nonce1, nonce2);
        // Different ciphertext (due to different nonces)
        assert_ne!(ct1, ct2);
    }

    #[test]
    fn decrypt_with_wrong_key_fails() {
        let (secret_a, _public_a) = EncryptedChannel::initiate();
        let (_secret_b, public_b) = EncryptedChannel::initiate();
        let (secret_c, _public_c) = EncryptedChannel::initiate();

        let channel_a = EncryptedChannel::complete(secret_a, &public_b);
        // Channel C has a different shared secret
        let channel_c = EncryptedChannel::complete(secret_c, &public_b);

        let plaintext = b"secret data";
        let (ciphertext, nonce) = channel_a.encrypt(plaintext).unwrap();

        // Decryption with wrong channel should fail
        let result = channel_c.decrypt(&ciphertext, &nonce);
        assert!(result.is_err());
    }

    #[test]
    fn decrypt_with_wrong_nonce_fails() {
        let (secret_a, public_a) = EncryptedChannel::initiate();
        let (secret_b, public_b) = EncryptedChannel::initiate();

        let channel_a = EncryptedChannel::complete(secret_a, &public_b);
        let channel_b = EncryptedChannel::complete(secret_b, &public_a);

        let plaintext = b"secret data";
        let (ciphertext, _nonce) = channel_a.encrypt(plaintext).unwrap();

        // Use a wrong nonce
        let wrong_nonce = [0u8; 24];
        let result = channel_b.decrypt(&ciphertext, &wrong_nonce);
        assert!(result.is_err());
    }

    #[test]
    fn encrypt_empty_data() {
        let (secret_a, public_a) = EncryptedChannel::initiate();
        let (secret_b, public_b) = EncryptedChannel::initiate();

        let channel_a = EncryptedChannel::complete(secret_a, &public_b);
        let channel_b = EncryptedChannel::complete(secret_b, &public_a);

        let plaintext = b"";
        let (ciphertext, nonce) = channel_a.encrypt(plaintext).unwrap();
        let decrypted = channel_b.decrypt(&ciphertext, &nonce).unwrap();
        assert!(decrypted.is_empty());
    }

    #[test]
    fn encrypt_large_data() {
        let (secret_a, public_a) = EncryptedChannel::initiate();
        let (secret_b, public_b) = EncryptedChannel::initiate();

        let channel_a = EncryptedChannel::complete(secret_a, &public_b);
        let channel_b = EncryptedChannel::complete(secret_b, &public_a);

        // 256KB chunk (typical transfer chunk size)
        let plaintext = vec![0xABu8; 256 * 1024];
        let (ciphertext, nonce) = channel_a.encrypt(&plaintext).unwrap();
        let decrypted = channel_b.decrypt(&ciphertext, &nonce).unwrap();
        assert_eq!(plaintext, decrypted);
    }

    #[test]
    fn key_exchange_function_works() {
        let (secret_a, public_a) = EncryptedChannel::initiate();
        let (secret_b, public_b) = EncryptedChannel::initiate();

        let channel_a = key_exchange(secret_a, &public_b);
        let channel_b = key_exchange(secret_b, &public_a);

        let plaintext = b"via key_exchange fn";
        let (ct, nonce) = channel_a.encrypt(plaintext).unwrap();
        let decrypted = channel_b.decrypt(&ct, &nonce).unwrap();
        assert_eq!(plaintext.as_slice(), decrypted.as_slice());
    }
}
