//! Alias store for named path aliases.
//!
//! Persists aliases in `aliases.toml` within the Flux config directory.
//! Provides CRUD operations and alias resolution for use in transfer commands.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::FluxError;

/// Serialized alias file format.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AliasFile {
    #[serde(default)]
    pub aliases: BTreeMap<String, String>,
}

/// In-memory representation of the alias store backed by a TOML file.
pub struct AliasStore {
    path: PathBuf,
    data: AliasFile,
}

/// Known URL schemes that must not be used as alias names.
const RESERVED_SCHEMES: &[&str] = &[
    "sftp", "ssh", "smb", "https", "http", "webdav", "dav", "ftp",
];

impl AliasStore {
    /// Load aliases from `aliases.toml` in the given config directory.
    ///
    /// Returns a default (empty) store if the file does not exist.
    pub fn load(config_dir: &Path) -> Result<Self, FluxError> {
        let path = config_dir.join("aliases.toml");
        let data = if path.exists() {
            let contents = std::fs::read_to_string(&path)?;
            toml::from_str(&contents)
                .map_err(|e| FluxError::Config(format!("Invalid aliases.toml: {}", e)))?
        } else {
            AliasFile::default()
        };
        Ok(Self { path, data })
    }

    /// Save aliases to disk atomically (write to tmp file, then rename).
    pub fn save(&self) -> Result<(), FluxError> {
        let contents = toml::to_string_pretty(&self.data)
            .map_err(|e| FluxError::Config(format!("Failed to serialize aliases: {}", e)))?;
        let tmp_path = self.path.with_extension("toml.tmp");
        std::fs::write(&tmp_path, &contents)?;
        std::fs::rename(&tmp_path, &self.path)?;
        Ok(())
    }

    /// Add or update an alias.
    pub fn add(&mut self, name: String, path: String) {
        self.data.aliases.insert(name, path);
    }

    /// Remove an alias by name. Returns whether it existed.
    pub fn remove(&mut self, name: &str) -> bool {
        self.data.aliases.remove(name).is_some()
    }

    /// Look up an alias by name.
    pub fn get(&self, name: &str) -> Option<&String> {
        self.data.aliases.get(name)
    }

    /// Return a reference to all stored aliases.
    pub fn list(&self) -> &BTreeMap<String, String> {
        &self.data.aliases
    }
}

impl Default for AliasStore {
    /// Create an empty alias store with no backing file.
    ///
    /// Used as a fallback when the config directory is not available.
    fn default() -> Self {
        Self {
            path: PathBuf::new(),
            data: AliasFile::default(),
        }
    }
}

/// Resolve alias references in a path string.
///
/// Patterns:
/// - `"name:"` -> base path from alias
/// - `"name:subpath"` -> base path + separator + subpath
/// - `"sftp://host/path"` -> unchanged (URL scheme)
/// - `"C:\path"` -> unchanged (drive letter)
/// - `"unknown-alias:"` -> unchanged (alias not found)
pub fn resolve_alias(input: &str, aliases: &AliasStore) -> String {
    if let Some(colon_pos) = input.find(':') {
        let name = &input[..colon_pos];
        let rest = &input[colon_pos + 1..];

        // Skip empty names
        if name.is_empty() {
            return input.to_string();
        }

        // Skip single-char names (Windows drive letters like C:)
        if name.len() == 1 {
            return input.to_string();
        }

        // Skip URL schemes (rest starts with "//")
        if rest.starts_with("//") {
            return input.to_string();
        }

        // Skip names containing path separators
        if name.contains('/') || name.contains('\\') {
            return input.to_string();
        }

        // Look up alias
        if let Some(base_path) = aliases.get(name) {
            if rest.is_empty() {
                return base_path.clone();
            }
            // Join with appropriate separator
            let separator = if base_path.contains('\\') {
                "\\"
            } else {
                "/"
            };
            return format!("{}{}{}", base_path, separator, rest);
        }
    }
    input.to_string()
}

/// Validate that an alias name is acceptable.
///
/// Rules:
/// - Must be at least 2 characters (reject single-char to avoid drive letter collision)
/// - Must not start with a digit
/// - Must contain only alphanumeric characters, hyphens, and underscores
/// - Must not match a known URL scheme (sftp, ssh, smb, https, http, webdav, dav, ftp)
pub fn validate_alias_name(name: &str) -> Result<(), FluxError> {
    if name.len() < 2 {
        return Err(FluxError::AliasError(
            "Alias name must be at least 2 characters (single characters conflict with drive letters)".into(),
        ));
    }

    if name.starts_with(|c: char| c.is_ascii_digit()) {
        return Err(FluxError::AliasError(
            "Alias name must not start with a digit".into(),
        ));
    }

    if !name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err(FluxError::AliasError(
            "Alias name must contain only alphanumeric characters, hyphens, and underscores".into(),
        ));
    }

    let lower = name.to_ascii_lowercase();
    if RESERVED_SCHEMES.contains(&lower.as_str()) {
        return Err(FluxError::AliasError(format!(
            "'{}' is a reserved protocol scheme and cannot be used as an alias name",
            name
        )));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn make_store(aliases: &[(&str, &str)]) -> AliasStore {
        let mut store = AliasStore::default();
        for (name, path) in aliases {
            store.add(name.to_string(), path.to_string());
        }
        store
    }

    // --- resolve_alias tests ---

    #[test]
    fn resolve_known_alias() {
        let store = make_store(&[("nas", "\\\\server\\share")]);
        assert_eq!(
            resolve_alias("nas:", &store),
            "\\\\server\\share"
        );
    }

    #[test]
    fn resolve_alias_with_subpath() {
        let store = make_store(&[("nas", "\\\\server\\share")]);
        assert_eq!(
            resolve_alias("nas:docs/readme.md", &store),
            "\\\\server\\share\\docs/readme.md"
        );
    }

    #[test]
    fn resolve_alias_with_forward_slash_base() {
        let store = make_store(&[("backup", "/mnt/backup")]);
        assert_eq!(
            resolve_alias("backup:photos/2024", &store),
            "/mnt/backup/photos/2024"
        );
    }

    #[test]
    fn resolve_unknown_alias_passthrough() {
        let store = make_store(&[("nas", "\\\\server\\share")]);
        assert_eq!(
            resolve_alias("unknown:path", &store),
            "unknown:path"
        );
    }

    #[test]
    fn resolve_url_scheme_passthrough() {
        let store = make_store(&[("sftp", "should-not-match")]);
        assert_eq!(
            resolve_alias("sftp://host/path", &store),
            "sftp://host/path"
        );
    }

    #[test]
    fn resolve_drive_letter_passthrough() {
        let store = make_store(&[]);
        assert_eq!(
            resolve_alias("C:\\Users\\test", &store),
            "C:\\Users\\test"
        );
    }

    #[test]
    fn resolve_empty_input_passthrough() {
        let store = make_store(&[]);
        assert_eq!(resolve_alias("", &store), "");
    }

    #[test]
    fn resolve_no_colon_passthrough() {
        let store = make_store(&[("nas", "\\\\server\\share")]);
        assert_eq!(
            resolve_alias("regular/path/file.txt", &store),
            "regular/path/file.txt"
        );
    }

    #[test]
    fn resolve_https_url_passthrough() {
        let store = make_store(&[("https", "should-not-match")]);
        assert_eq!(
            resolve_alias("https://cloud.example.com/dav/", &store),
            "https://cloud.example.com/dav/"
        );
    }

    // --- validate_alias_name tests ---

    #[test]
    fn validate_good_names() {
        assert!(validate_alias_name("nas").is_ok());
        assert!(validate_alias_name("my-server").is_ok());
        assert!(validate_alias_name("backup_2024").is_ok());
        assert!(validate_alias_name("ab").is_ok());
    }

    #[test]
    fn validate_rejects_single_char() {
        assert!(validate_alias_name("a").is_err());
        assert!(validate_alias_name("C").is_err());
    }

    #[test]
    fn validate_rejects_empty() {
        assert!(validate_alias_name("").is_err());
    }

    #[test]
    fn validate_rejects_digit_start() {
        assert!(validate_alias_name("1nas").is_err());
    }

    #[test]
    fn validate_rejects_reserved_schemes() {
        assert!(validate_alias_name("sftp").is_err());
        assert!(validate_alias_name("ssh").is_err());
        assert!(validate_alias_name("smb").is_err());
        assert!(validate_alias_name("https").is_err());
        assert!(validate_alias_name("http").is_err());
        assert!(validate_alias_name("webdav").is_err());
        assert!(validate_alias_name("dav").is_err());
        assert!(validate_alias_name("ftp").is_err());
        // Case insensitive
        assert!(validate_alias_name("SFTP").is_err());
    }

    #[test]
    fn validate_rejects_special_chars() {
        assert!(validate_alias_name("my server").is_err());
        assert!(validate_alias_name("nas.local").is_err());
        assert!(validate_alias_name("path/name").is_err());
    }

    // --- AliasStore persistence tests ---

    #[test]
    fn store_load_save_roundtrip() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path();

        // Load empty store
        let mut store = AliasStore::load(dir).unwrap();
        assert!(store.list().is_empty());

        // Add aliases
        store.add("nas".to_string(), "\\\\server\\share".to_string());
        store.add("backup".to_string(), "/mnt/backup".to_string());
        store.save().unwrap();

        // Reload and verify
        let store2 = AliasStore::load(dir).unwrap();
        assert_eq!(store2.get("nas"), Some(&"\\\\server\\share".to_string()));
        assert_eq!(store2.get("backup"), Some(&"/mnt/backup".to_string()));
        assert_eq!(store2.list().len(), 2);
    }

    #[test]
    fn store_remove_alias() {
        let mut store = make_store(&[("nas", "path"), ("backup", "other")]);
        assert!(store.remove("nas"));
        assert!(!store.remove("nonexistent"));
        assert!(store.get("nas").is_none());
        assert_eq!(store.list().len(), 1);
    }

    #[test]
    fn store_default_is_empty() {
        let store = AliasStore::default();
        assert!(store.list().is_empty());
    }
}
