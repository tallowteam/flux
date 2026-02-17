use serde::{Deserialize, Serialize};

use crate::error::FluxError;

/// Verbosity level controlling tracing output.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Verbosity {
    /// Suppress all output except errors
    Quiet,
    /// Normal output (info level)
    Normal,
    /// Verbose output (debug level)
    Verbose,
    /// Maximum output (trace level)
    Trace,
}

impl From<(bool, u8)> for Verbosity {
    /// Convert from (quiet_flag, verbose_count) to Verbosity.
    ///
    /// - quiet=true -> Quiet (regardless of verbose count)
    /// - verbose=0  -> Normal
    /// - verbose=1  -> Verbose
    /// - verbose=2+ -> Trace
    fn from((quiet, verbose_count): (bool, u8)) -> Self {
        if quiet {
            Verbosity::Quiet
        } else {
            match verbose_count {
                0 => Verbosity::Normal,
                1 => Verbosity::Verbose,
                _ => Verbosity::Trace,
            }
        }
    }
}

impl Verbosity {
    /// Return the tracing filter string for this verbosity level.
    pub fn as_tracing_filter(&self) -> &'static str {
        match self {
            Verbosity::Quiet => "error",
            Verbosity::Normal => "info",
            Verbosity::Verbose => "debug",
            Verbosity::Trace => "trace",
        }
    }
}

/// Strategy for handling file conflicts when the destination already exists.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, clap::ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum ConflictStrategy {
    /// Overwrite the existing file
    Overwrite,
    /// Skip the file (do not copy)
    Skip,
    /// Rename the new file with a numeric suffix (e.g., file_1.txt)
    Rename,
    /// Ask the user interactively (falls back to Skip if stdin is not a TTY)
    Ask,
}

/// Strategy for handling failures during file copy operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, clap::ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum FailureStrategy {
    /// Retry the operation with exponential backoff
    Retry,
    /// Skip the failed file and continue
    Skip,
    /// Pause and prompt the user before continuing
    Pause,
}

/// Application configuration loaded from config.toml with serde defaults.
///
/// All fields have sensible defaults so a partial or missing config.toml
/// works without errors.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct FluxConfig {
    pub verbosity: Verbosity,
    pub conflict: ConflictStrategy,
    pub failure: FailureStrategy,
    pub retry_count: u32,
    pub retry_backoff_ms: u64,
    pub default_destination: Option<String>,
    pub history_limit: usize,
}

impl Default for FluxConfig {
    fn default() -> Self {
        Self {
            verbosity: Verbosity::Normal,
            conflict: ConflictStrategy::Ask,
            failure: FailureStrategy::Retry,
            retry_count: 3,
            retry_backoff_ms: 1000,
            default_destination: None,
            history_limit: 1000,
        }
    }
}

/// Load config from disk. Returns defaults if the file does not exist.
///
/// Config is NOT auto-created on first run. Only written when the user
/// explicitly configures something. Invalid TOML produces a Config error.
pub fn load_config() -> Result<FluxConfig, FluxError> {
    let config_dir = crate::config::paths::flux_config_dir()?;
    let config_path = config_dir.join("config.toml");
    if config_path.exists() {
        let contents = std::fs::read_to_string(&config_path)?;
        let config: FluxConfig = toml::from_str(&contents)
            .map_err(|e| FluxError::Config(format!("Invalid config.toml: {}", e)))?;
        Ok(config)
    } else {
        Ok(FluxConfig::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_has_expected_values() {
        let config = FluxConfig::default();
        assert_eq!(config.conflict, ConflictStrategy::Ask);
        assert_eq!(config.failure, FailureStrategy::Retry);
        assert_eq!(config.retry_count, 3);
        assert_eq!(config.retry_backoff_ms, 1000);
        assert!(config.default_destination.is_none());
        assert_eq!(config.history_limit, 1000);
        assert_eq!(config.verbosity, Verbosity::Normal);
    }

    #[test]
    fn toml_roundtrip_preserves_values() {
        let config = FluxConfig {
            verbosity: Verbosity::Verbose,
            conflict: ConflictStrategy::Skip,
            failure: FailureStrategy::Pause,
            retry_count: 5,
            retry_backoff_ms: 2000,
            default_destination: Some("/tmp/dest".to_string()),
            history_limit: 500,
        };
        let toml_str = toml::to_string_pretty(&config).expect("serialize");
        let loaded: FluxConfig = toml::from_str(&toml_str).expect("deserialize");
        assert_eq!(loaded.conflict, ConflictStrategy::Skip);
        assert_eq!(loaded.failure, FailureStrategy::Pause);
        assert_eq!(loaded.retry_count, 5);
        assert_eq!(loaded.retry_backoff_ms, 2000);
        assert_eq!(loaded.default_destination, Some("/tmp/dest".to_string()));
        assert_eq!(loaded.history_limit, 500);
        assert_eq!(loaded.verbosity, Verbosity::Verbose);
    }

    #[test]
    fn partial_toml_fills_defaults() {
        let partial = r#"
conflict = "overwrite"
retry_count = 10
"#;
        let config: FluxConfig = toml::from_str(partial).expect("parse partial");
        assert_eq!(config.conflict, ConflictStrategy::Overwrite);
        assert_eq!(config.retry_count, 10);
        // Defaults for missing fields
        assert_eq!(config.failure, FailureStrategy::Retry);
        assert_eq!(config.retry_backoff_ms, 1000);
        assert!(config.default_destination.is_none());
        assert_eq!(config.history_limit, 1000);
        assert_eq!(config.verbosity, Verbosity::Normal);
    }

    #[test]
    fn load_config_returns_defaults_when_no_file() {
        // load_config should not fail when no config.toml exists
        let config = load_config().expect("should return defaults");
        assert_eq!(config.conflict, ConflictStrategy::Ask);
        assert_eq!(config.failure, FailureStrategy::Retry);
    }

    #[test]
    fn verbosity_from_flags_unchanged() {
        assert_eq!(Verbosity::from((true, 0)), Verbosity::Quiet);
        assert_eq!(Verbosity::from((false, 0)), Verbosity::Normal);
        assert_eq!(Verbosity::from((false, 1)), Verbosity::Verbose);
        assert_eq!(Verbosity::from((false, 2)), Verbosity::Trace);
    }

    #[test]
    fn conflict_strategy_all_variants_roundtrip() {
        // TOML can't serialize bare enums, so wrap in a struct
        #[derive(Debug, Serialize, Deserialize, PartialEq)]
        struct Wrapper { conflict: ConflictStrategy }

        for strategy in &[
            ConflictStrategy::Overwrite,
            ConflictStrategy::Skip,
            ConflictStrategy::Rename,
            ConflictStrategy::Ask,
        ] {
            let w = Wrapper { conflict: *strategy };
            let toml_str = toml::to_string(&w).expect("serialize");
            let loaded: Wrapper = toml::from_str(&toml_str).expect("deserialize");
            assert_eq!(w, loaded);
        }
    }

    #[test]
    fn failure_strategy_all_variants_roundtrip() {
        #[derive(Debug, Serialize, Deserialize, PartialEq)]
        struct Wrapper { failure: FailureStrategy }

        for strategy in &[
            FailureStrategy::Retry,
            FailureStrategy::Skip,
            FailureStrategy::Pause,
        ] {
            let w = Wrapper { failure: *strategy };
            let toml_str = toml::to_string(&w).expect("serialize");
            let loaded: Wrapper = toml::from_str(&toml_str).expect("deserialize");
            assert_eq!(w, loaded);
        }
    }
}
