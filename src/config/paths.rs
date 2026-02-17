//! Platform-specific config and data directory helpers.
//!
//! Uses the `dirs` crate to resolve platform-appropriate directories:
//! - Linux:   `~/.config/flux/` (config), `~/.local/share/flux/` (data)
//! - Windows: `%APPDATA%\flux\` (config), `%APPDATA%\flux\` (data)
//! - macOS:   `~/Library/Application Support/flux/` (both)

use std::path::PathBuf;

use crate::error::FluxError;

/// Get the Flux config directory, creating it if needed.
///
/// Returns the platform-specific config directory with a `flux` subdirectory.
/// Creates the directory if it does not exist.
///
/// If the `FLUX_CONFIG_DIR` environment variable is set, it overrides the
/// default location. This enables test isolation and custom deployments.
pub fn flux_config_dir() -> Result<PathBuf, FluxError> {
    if let Ok(override_dir) = std::env::var("FLUX_CONFIG_DIR") {
        let dir = PathBuf::from(override_dir);
        if !dir.exists() {
            std::fs::create_dir_all(&dir)?;
        }
        return Ok(dir);
    }
    let base = dirs::config_dir()
        .ok_or_else(|| FluxError::Config("Could not determine config directory".into()))?;
    let flux_dir = base.join("flux");
    if !flux_dir.exists() {
        std::fs::create_dir_all(&flux_dir)?;
        // Restrict config directory permissions on Unix (contains identity.json private key)
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&flux_dir, std::fs::Permissions::from_mode(0o700)).ok();
        }
    }
    Ok(flux_dir)
}

/// Get the Flux data directory, creating it if needed.
///
/// Returns the platform-specific data directory with a `flux` subdirectory.
/// Used for data files like queue state and transfer history.
///
/// If the `FLUX_DATA_DIR` environment variable is set, it overrides the
/// default location. This enables test isolation and custom deployments.
pub fn flux_data_dir() -> Result<PathBuf, FluxError> {
    if let Ok(override_dir) = std::env::var("FLUX_DATA_DIR") {
        let dir = PathBuf::from(override_dir);
        if !dir.exists() {
            std::fs::create_dir_all(&dir)?;
        }
        return Ok(dir);
    }
    let base = dirs::data_dir()
        .ok_or_else(|| FluxError::Config("Could not determine data directory".into()))?;
    let flux_dir = base.join("flux");
    if !flux_dir.exists() {
        std::fs::create_dir_all(&flux_dir)?;
    }
    Ok(flux_dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_dir_returns_path_containing_flux() {
        let dir = flux_config_dir().expect("should resolve config dir");
        assert!(dir.ends_with("flux"));
        assert!(dir.exists());
    }

    #[test]
    fn data_dir_returns_path_containing_flux() {
        let dir = flux_data_dir().expect("should resolve data dir");
        assert!(dir.ends_with("flux"));
        assert!(dir.exists());
    }
}
