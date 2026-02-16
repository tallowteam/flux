//! Protocol detection and path parsing for network backends.
//!
//! This module provides the `Protocol` enum that represents the detected
//! transfer protocol from a user-provided path or URI, along with parsing
//! logic and authentication types.

pub mod auth;
pub mod parser;

use std::path::PathBuf;

pub use auth::Auth;
pub use parser::detect_protocol;

/// A detected transfer protocol with parsed connection parameters.
///
/// Created by `detect_protocol()` from a raw user input string.
/// Used by `create_backend()` in `backend/mod.rs` to instantiate
/// the appropriate `FluxBackend` implementation.
#[derive(Debug, Clone)]
pub enum Protocol {
    /// Local filesystem path.
    Local { path: PathBuf },

    /// SFTP (SSH File Transfer Protocol).
    Sftp {
        user: String,
        host: String,
        port: u16,
        path: String,
    },

    /// SMB/CIFS network share.
    Smb {
        server: String,
        share: String,
        path: String,
    },

    /// WebDAV (HTTP-based file access).
    WebDav {
        url: String,
        auth: Option<Auth>,
    },
}

impl Protocol {
    /// Returns true if this is a local filesystem protocol.
    pub fn is_local(&self) -> bool {
        matches!(self, Protocol::Local { .. })
    }

    /// Extract the local path if this is a Local protocol.
    /// Returns None for network protocols.
    pub fn local_path(&self) -> Option<&PathBuf> {
        match self {
            Protocol::Local { path } => Some(path),
            _ => None,
        }
    }

    /// Returns a human-readable protocol name.
    pub fn name(&self) -> &'static str {
        match self {
            Protocol::Local { .. } => "local",
            Protocol::Sftp { .. } => "sftp",
            Protocol::Smb { .. } => "smb",
            Protocol::WebDav { .. } => "webdav",
        }
    }
}
