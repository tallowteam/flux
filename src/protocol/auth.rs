//! Authentication types for network protocol backends.
//!
//! Skeleton for Phase 5 (Security). Currently defines the Auth enum
//! used by Protocol variants to carry optional credentials.

use std::path::PathBuf;

/// Authentication method for network connections.
#[derive(Clone)]
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

// Custom Debug implementation that redacts passwords and passphrases
// to prevent credential leakage in log output.
impl std::fmt::Debug for Auth {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Auth::None => write!(f, "Auth::None"),
            Auth::Password { user, .. } => f
                .debug_struct("Auth::Password")
                .field("user", user)
                .field("password", &"[REDACTED]")
                .finish(),
            Auth::KeyFile {
                user, key_path, passphrase,
            } => f
                .debug_struct("Auth::KeyFile")
                .field("user", user)
                .field("key_path", key_path)
                .field("passphrase", if passphrase.is_some() {
                    &"Some([REDACTED])"
                } else {
                    &"None"
                })
                .finish(),
            Auth::Agent { user } => f
                .debug_struct("Auth::Agent")
                .field("user", user)
                .finish(),
        }
    }
}
