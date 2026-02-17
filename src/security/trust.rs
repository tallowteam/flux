//! Trust-on-First-Use (TOFU) device store for Flux peer authentication.
//!
//! Stores known device public keys in a JSON file (`trusted_devices.json`)
//! in the config directory. On first connection, the user decides whether
//! to trust the peer. On subsequent connections, the stored key is verified.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use subtle::ConstantTimeEq;

use crate::error::FluxError;

/// Result of verifying a device against the trust store.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrustStatus {
    /// The device is known and its public key matches.
    Trusted,
    /// The device is not in the trust store.
    Unknown,
    /// The device is known but its public key has changed.
    /// This may indicate a key rotation or an impersonation attempt.
    KeyChanged,
}

/// A trusted device record.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct TrustedDevice {
    /// Base64-encoded X25519 public key.
    pub public_key: String,
    /// When this device was first trusted.
    pub first_seen: DateTime<Utc>,
    /// When this device was last seen.
    pub last_seen: DateTime<Utc>,
    /// Human-readable name for the device.
    pub friendly_name: String,
}

/// Persistent store for trusted device public keys.
///
/// Backed by a JSON file at `config_dir/trusted_devices.json`.
/// Uses atomic writes (write to `.tmp`, then rename) for crash safety.
#[derive(Serialize, Deserialize)]
pub struct TrustStore {
    devices: BTreeMap<String, TrustedDevice>,
    #[serde(skip)]
    path: PathBuf,
}

impl TrustStore {
    /// Load the trust store from `config_dir/trusted_devices.json`.
    /// Returns an empty store if the file does not exist.
    /// Silently starts fresh if the file is corrupted (matches Flux pattern).
    pub fn load(config_dir: &Path) -> Result<Self, FluxError> {
        let path = config_dir.join("trusted_devices.json");

        if path.exists() {
            let data = std::fs::read_to_string(&path).map_err(|e| {
                FluxError::TrustError(format!("Failed to read trust store: {}", e))
            })?;
            match serde_json::from_str::<TrustStore>(&data) {
                Ok(mut store) => {
                    store.path = path;
                    Ok(store)
                }
                Err(e) => {
                    // Corrupted file: warn the user and start fresh.
                    // This is logged as a warning because a corrupted trust store
                    // means all previously trusted devices will need re-verification.
                    tracing::warn!(
                        "Trust store corrupted ({}), starting fresh. \
                         Previously trusted devices will need re-verification.",
                        e
                    );
                    Ok(Self {
                        devices: BTreeMap::new(),
                        path,
                    })
                }
            }
        } else {
            Ok(Self {
                devices: BTreeMap::new(),
                path,
            })
        }
    }

    /// Save the trust store to disk using atomic write.
    pub fn save(&self) -> Result<(), FluxError> {
        let tmp_path = self.path.with_extension("json.tmp");

        let json = serde_json::to_string_pretty(&self).map_err(|e| {
            FluxError::TrustError(format!("Failed to serialize trust store: {}", e))
        })?;

        std::fs::write(&tmp_path, &json).map_err(|e| {
            FluxError::TrustError(format!("Failed to write trust store: {}", e))
        })?;
        std::fs::rename(&tmp_path, &self.path).map_err(|e| {
            FluxError::TrustError(format!("Failed to save trust store: {}", e))
        })?;

        Ok(())
    }

    /// Check if a device is trusted.
    ///
    /// Returns:
    /// - `Trusted` if the device name exists and the public key matches.
    /// - `Unknown` if the device name is not in the store.
    /// - `KeyChanged` if the device name exists but the public key differs.
    ///
    /// Public key comparison uses constant-time equality to prevent timing
    /// side-channel leaks that could reveal information about stored keys.
    pub fn is_trusted(&self, device_name: &str, public_key_b64: &str) -> TrustStatus {
        match self.devices.get(device_name) {
            None => TrustStatus::Unknown,
            Some(device) => {
                let stored = device.public_key.as_bytes();
                let provided = public_key_b64.as_bytes();
                // Constant-time comparison: check length equality first (not secret),
                // then compare bytes in constant time to avoid timing leaks on key content.
                if stored.len() == provided.len()
                    && stored.ct_eq(provided).into()
                {
                    TrustStatus::Trusted
                } else {
                    TrustStatus::KeyChanged
                }
            }
        }
    }

    /// Add or update a device in the trust store.
    ///
    /// If the device already exists, its public key and `last_seen` are updated.
    /// If the device is new, `first_seen` and `last_seen` are both set to now.
    pub fn add_device(&mut self, name: String, public_key: String, friendly_name: String) {
        let now = Utc::now();
        if let Some(existing) = self.devices.get_mut(&name) {
            existing.public_key = public_key;
            existing.last_seen = now;
            existing.friendly_name = friendly_name;
        } else {
            self.devices.insert(
                name,
                TrustedDevice {
                    public_key,
                    first_seen: now,
                    last_seen: now,
                    friendly_name,
                },
            );
        }
    }

    /// Remove a device from the trust store.
    /// Returns `true` if the device was found and removed.
    pub fn remove_device(&mut self, name: &str) -> bool {
        self.devices.remove(name).is_some()
    }

    /// List all trusted devices, sorted by name.
    pub fn list_devices(&self) -> Vec<(&String, &TrustedDevice)> {
        self.devices.iter().collect()
    }

    /// Return the number of trusted devices.
    pub fn device_count(&self) -> usize {
        self.devices.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_empty_store_when_file_missing() {
        let dir = tempfile::tempdir().unwrap();
        let store = TrustStore::load(dir.path()).unwrap();
        assert_eq!(store.device_count(), 0);
        assert!(store.list_devices().is_empty());
    }

    #[test]
    fn add_and_list_devices() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = TrustStore::load(dir.path()).unwrap();

        store.add_device(
            "alice-laptop".into(),
            "AAAA".into(),
            "Alice's Laptop".into(),
        );
        store.add_device(
            "bob-desktop".into(),
            "BBBB".into(),
            "Bob's Desktop".into(),
        );

        assert_eq!(store.device_count(), 2);
        let devices = store.list_devices();
        // BTreeMap is sorted by key
        assert_eq!(devices[0].0, "alice-laptop");
        assert_eq!(devices[1].0, "bob-desktop");
        assert_eq!(devices[0].1.friendly_name, "Alice's Laptop");
    }

    #[test]
    fn is_trusted_returns_correct_status() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = TrustStore::load(dir.path()).unwrap();

        store.add_device("server".into(), "KEY123".into(), "My Server".into());

        // Known device with matching key
        assert_eq!(
            store.is_trusted("server", "KEY123"),
            TrustStatus::Trusted
        );

        // Unknown device
        assert_eq!(
            store.is_trusted("unknown-device", "KEY456"),
            TrustStatus::Unknown
        );

        // Known device with changed key
        assert_eq!(
            store.is_trusted("server", "DIFFERENT_KEY"),
            TrustStatus::KeyChanged
        );
    }

    #[test]
    fn remove_device_works() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = TrustStore::load(dir.path()).unwrap();

        store.add_device("device1".into(), "K1".into(), "Device 1".into());
        assert_eq!(store.device_count(), 1);

        // Remove existing
        assert!(store.remove_device("device1"));
        assert_eq!(store.device_count(), 0);

        // Remove non-existing
        assert!(!store.remove_device("device1"));
    }

    #[test]
    fn save_and_reload_persists_data() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = TrustStore::load(dir.path()).unwrap();

        store.add_device("laptop".into(), "PK_ABC".into(), "My Laptop".into());
        store.save().unwrap();

        // Reload from disk
        let store2 = TrustStore::load(dir.path()).unwrap();
        assert_eq!(store2.device_count(), 1);
        assert_eq!(
            store2.is_trusted("laptop", "PK_ABC"),
            TrustStatus::Trusted
        );
        let devices = store2.list_devices();
        assert_eq!(devices[0].1.friendly_name, "My Laptop");
    }

    #[test]
    fn corrupted_file_starts_fresh() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("trusted_devices.json"),
            "not valid json!!!",
        )
        .unwrap();

        let store = TrustStore::load(dir.path()).unwrap();
        assert_eq!(store.device_count(), 0);
    }

    #[test]
    fn add_device_updates_existing() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = TrustStore::load(dir.path()).unwrap();

        store.add_device("dev".into(), "OLD_KEY".into(), "Old Name".into());
        let first_seen = store.list_devices()[0].1.first_seen;

        // Small delay to ensure different timestamp
        std::thread::sleep(std::time::Duration::from_millis(10));

        store.add_device("dev".into(), "NEW_KEY".into(), "New Name".into());
        assert_eq!(store.device_count(), 1);

        let devices = store.list_devices();
        let device = devices[0].1;
        assert_eq!(device.public_key, "NEW_KEY");
        assert_eq!(device.friendly_name, "New Name");
        // first_seen should remain unchanged
        assert_eq!(device.first_seen, first_seen);
        // last_seen should be updated
        assert!(device.last_seen >= device.first_seen);
    }

    #[test]
    fn key_changed_detection() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = TrustStore::load(dir.path()).unwrap();

        // Trust a device with key A
        store.add_device("peer".into(), "KEY_A".into(), "Peer".into());
        assert_eq!(store.is_trusted("peer", "KEY_A"), TrustStatus::Trusted);

        // Same device presenting key B => KeyChanged (possible impersonation)
        assert_eq!(
            store.is_trusted("peer", "KEY_B"),
            TrustStatus::KeyChanged
        );
    }

    #[test]
    fn list_devices_sorted_by_name() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = TrustStore::load(dir.path()).unwrap();

        store.add_device("zeta".into(), "Z".into(), "Zeta".into());
        store.add_device("alpha".into(), "A".into(), "Alpha".into());
        store.add_device("mid".into(), "M".into(), "Mid".into());

        let devices = store.list_devices();
        let names: Vec<&str> = devices.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, vec!["alpha", "mid", "zeta"]);
    }
}
