//! Authentication types for network protocol backends.
//!
//! Skeleton for Phase 5 (Security). Currently defines the Auth enum
//! used by Protocol variants to carry optional credentials.

use std::path::PathBuf;

/// Authentication method for network connections.
#[derive(Clone, Debug)]
pub enum Auth {
    /// No authentication (anonymous access).
    None,

    /// Username + password authentication.
    Password {
        user: String,
        password: String,
    },

    /// SSH key file authentication.
    KeyFile {
        user: String,
        key_path: PathBuf,
        passphrase: Option<String>,
    },

    /// SSH agent forwarded authentication.
    Agent {
        user: String,
    },
}
