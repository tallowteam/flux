//! Protocol detection from user-provided path/URI strings.
//!
//! Parses raw input strings into a `Protocol` enum indicating which backend
//! to use for the transfer. Supports local paths, UNC paths, and URL schemes.

use std::path::PathBuf;

use url::Url;

use super::auth::Auth;
use super::Protocol;

/// Detect the transfer protocol from a raw input string.
///
/// Detection order:
/// 1. Windows UNC path: `\\server\share\path` -> SMB
/// 2. Unix UNC path: `//server/share/path` (but not `///`) -> SMB
/// 3. URL with recognized scheme (`sftp`, `ssh`, `smb`, `https`, `http`, `webdav`, `dav`) -> respective protocol
/// 4. Everything else -> Local
pub fn detect_protocol(input: &str) -> Protocol {
    // 1. Windows UNC path: \\server\share\path
    if input.starts_with("\\\\") {
        return parse_unc_backslash(input);
    }

    // 2. Unix-style UNC path: //server/share/path (but not /// which is a local path)
    if input.starts_with("//") && !input.starts_with("///") {
        return parse_unc_forward(input);
    }

    // 3. Try URL parsing for scheme-based detection
    if let Ok(url) = Url::parse(input) {
        match url.scheme() {
            "sftp" | "ssh" => return parse_sftp_url(&url),
            "smb" => return parse_smb_url(&url),
            "https" | "http" | "webdav" | "dav" => {
                return Protocol::WebDav {
                    url: input.to_string(),
                    auth: extract_webdav_auth(&url),
                };
            }
            _ => {
                // On Windows, single drive letters like C: are parsed as URL schemes.
                // If the scheme is a single ASCII letter, treat it as a local path.
                if url.scheme().len() == 1 && url.scheme().chars().next().map_or(false, |c| c.is_ascii_alphabetic()) {
                    return Protocol::Local {
                        path: PathBuf::from(input),
                    };
                }
            }
        }
    }

    // 4. Fallback: local filesystem path
    Protocol::Local {
        path: PathBuf::from(input),
    }
}

/// Parse a Windows-style UNC path: `\\server\share\path`
fn parse_unc_backslash(input: &str) -> Protocol {
    let trimmed = input.trim_start_matches('\\');
    let parts: Vec<&str> = trimmed.splitn(3, '\\').collect();
    match parts.len() {
        0 | 1 => Protocol::Smb {
            server: parts.first().unwrap_or(&"").to_string(),
            share: String::new(),
            path: String::new(),
        },
        2 => Protocol::Smb {
            server: parts[0].to_string(),
            share: parts[1].to_string(),
            path: String::new(),
        },
        _ => Protocol::Smb {
            server: parts[0].to_string(),
            share: parts[1].to_string(),
            path: parts[2].to_string(),
        },
    }
}

/// Parse a Unix-style UNC path: `//server/share/path`
fn parse_unc_forward(input: &str) -> Protocol {
    let trimmed = input.trim_start_matches('/');
    let parts: Vec<&str> = trimmed.splitn(3, '/').collect();
    match parts.len() {
        0 | 1 => Protocol::Smb {
            server: parts.first().unwrap_or(&"").to_string(),
            share: String::new(),
            path: String::new(),
        },
        2 => Protocol::Smb {
            server: parts[0].to_string(),
            share: parts[1].to_string(),
            path: String::new(),
        },
        _ => Protocol::Smb {
            server: parts[0].to_string(),
            share: parts[1].to_string(),
            path: parts[2].to_string(),
        },
    }
}

/// Parse an SFTP/SSH URL into Protocol::Sftp.
fn parse_sftp_url(url: &Url) -> Protocol {
    let user = if url.username().is_empty() {
        String::new()
    } else {
        url.username().to_string()
    };

    let host = url.host_str().unwrap_or("").to_string();
    let port = url.port().unwrap_or(22);
    let path = url.path().to_string();

    Protocol::Sftp {
        user,
        host,
        port,
        path,
    }
}

/// Parse an SMB URL (`smb://server/share/path`) into Protocol::Smb.
fn parse_smb_url(url: &Url) -> Protocol {
    let server = url.host_str().unwrap_or("").to_string();
    let url_path = url.path().trim_start_matches('/');

    let (share, path) = if let Some(idx) = url_path.find('/') {
        (url_path[..idx].to_string(), url_path[idx + 1..].to_string())
    } else {
        (url_path.to_string(), String::new())
    };

    Protocol::Smb {
        server,
        share,
        path,
    }
}

/// Extract inline WebDAV credentials from URL userinfo, if present.
fn extract_webdav_auth(url: &Url) -> Option<Auth> {
    let user = url.username();
    if user.is_empty() {
        return None;
    }

    let password = url.password().unwrap_or("").to_string();
    if password.is_empty() {
        Some(Auth::Password {
            user: user.to_string(),
            password: String::new(),
        })
    } else {
        Some(Auth::Password {
            user: user.to_string(),
            password,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_local_relative_path() {
        let proto = detect_protocol("myfile.txt");
        match proto {
            Protocol::Local { path } => assert_eq!(path, PathBuf::from("myfile.txt")),
            other => panic!("Expected Local, got {:?}", other),
        }
    }

    #[test]
    fn detect_local_absolute_unix_path() {
        let proto = detect_protocol("/home/user/file.txt");
        match proto {
            Protocol::Local { path } => assert_eq!(path, PathBuf::from("/home/user/file.txt")),
            other => panic!("Expected Local, got {:?}", other),
        }
    }

    #[test]
    fn detect_local_absolute_windows_path() {
        let proto = detect_protocol("C:\\Users\\test\\file.txt");
        match proto {
            Protocol::Local { path } => {
                assert_eq!(path, PathBuf::from("C:\\Users\\test\\file.txt"));
            }
            other => panic!("Expected Local, got {:?}", other),
        }
    }

    #[test]
    fn detect_windows_unc_path() {
        let proto = detect_protocol("\\\\server\\share\\docs\\file.txt");
        match proto {
            Protocol::Smb {
                server,
                share,
                path,
            } => {
                assert_eq!(server, "server");
                assert_eq!(share, "share");
                assert_eq!(path, "docs\\file.txt");
            }
            other => panic!("Expected Smb, got {:?}", other),
        }
    }

    #[test]
    fn detect_unix_unc_path() {
        let proto = detect_protocol("//server/share/docs/file.txt");
        match proto {
            Protocol::Smb {
                server,
                share,
                path,
            } => {
                assert_eq!(server, "server");
                assert_eq!(share, "share");
                assert_eq!(path, "docs/file.txt");
            }
            other => panic!("Expected Smb, got {:?}", other),
        }
    }

    #[test]
    fn detect_sftp_with_user() {
        let proto = detect_protocol("sftp://alice@myhost.com/home/alice/data.bin");
        match proto {
            Protocol::Sftp {
                user,
                host,
                port,
                path,
            } => {
                assert_eq!(user, "alice");
                assert_eq!(host, "myhost.com");
                assert_eq!(port, 22);
                assert_eq!(path, "/home/alice/data.bin");
            }
            other => panic!("Expected Sftp, got {:?}", other),
        }
    }

    #[test]
    fn detect_sftp_without_user() {
        let proto = detect_protocol("sftp://myhost.com/path/to/file");
        match proto {
            Protocol::Sftp {
                user,
                host,
                port,
                path,
            } => {
                assert_eq!(user, "");
                assert_eq!(host, "myhost.com");
                assert_eq!(port, 22);
                assert_eq!(path, "/path/to/file");
            }
            other => panic!("Expected Sftp, got {:?}", other),
        }
    }

    #[test]
    fn detect_sftp_with_port() {
        let proto = detect_protocol("sftp://bob@server.io:2222/data");
        match proto {
            Protocol::Sftp {
                user,
                host,
                port,
                path,
            } => {
                assert_eq!(user, "bob");
                assert_eq!(host, "server.io");
                assert_eq!(port, 2222);
                assert_eq!(path, "/data");
            }
            other => panic!("Expected Sftp, got {:?}", other),
        }
    }

    #[test]
    fn detect_ssh_scheme_as_sftp() {
        let proto = detect_protocol("ssh://user@host/path");
        match proto {
            Protocol::Sftp { user, host, .. } => {
                assert_eq!(user, "user");
                assert_eq!(host, "host");
            }
            other => panic!("Expected Sftp, got {:?}", other),
        }
    }

    #[test]
    fn detect_smb_url() {
        let proto = detect_protocol("smb://fileserver/shared/docs/readme.md");
        match proto {
            Protocol::Smb {
                server,
                share,
                path,
            } => {
                assert_eq!(server, "fileserver");
                assert_eq!(share, "shared");
                assert_eq!(path, "docs/readme.md");
            }
            other => panic!("Expected Smb, got {:?}", other),
        }
    }

    #[test]
    fn detect_https_webdav() {
        let proto = detect_protocol("https://cloud.example.com/webdav/folder/");
        match proto {
            Protocol::WebDav { url, auth } => {
                assert_eq!(url, "https://cloud.example.com/webdav/folder/");
                assert!(auth.is_none());
            }
            other => panic!("Expected WebDav, got {:?}", other),
        }
    }

    #[test]
    fn detect_http_webdav() {
        let proto = detect_protocol("http://nas.local:5005/webdav/files");
        match proto {
            Protocol::WebDav { url, .. } => {
                assert_eq!(url, "http://nas.local:5005/webdav/files");
            }
            other => panic!("Expected WebDav, got {:?}", other),
        }
    }

    #[test]
    fn detect_dav_scheme() {
        let proto = detect_protocol("dav://server.com/share/");
        match proto {
            Protocol::WebDav { url, .. } => {
                assert_eq!(url, "dav://server.com/share/");
            }
            other => panic!("Expected WebDav, got {:?}", other),
        }
    }

    #[test]
    fn detect_webdav_scheme() {
        let proto = detect_protocol("webdav://server.com/share/");
        match proto {
            Protocol::WebDav { url, .. } => {
                assert_eq!(url, "webdav://server.com/share/");
            }
            other => panic!("Expected WebDav, got {:?}", other),
        }
    }

    #[test]
    fn detect_webdav_with_inline_credentials() {
        let proto = detect_protocol("https://admin:secret@server.com/dav/");
        match proto {
            Protocol::WebDav { auth, .. } => {
                match auth {
                    Some(Auth::Password { user, password }) => {
                        assert_eq!(user, "admin");
                        assert_eq!(password, "secret");
                    }
                    other => panic!("Expected Password auth, got {:?}", other),
                }
            }
            other => panic!("Expected WebDav, got {:?}", other),
        }
    }

    #[test]
    fn detect_local_path_that_looks_like_url() {
        // A path like "file.sftp" should not be detected as SFTP
        let proto = detect_protocol("file.sftp");
        match proto {
            Protocol::Local { .. } => {}
            other => panic!("Expected Local, got {:?}", other),
        }
    }

    #[test]
    fn detect_unc_server_only() {
        let proto = detect_protocol("\\\\server");
        match proto {
            Protocol::Smb { server, share, path } => {
                assert_eq!(server, "server");
                assert_eq!(share, "");
                assert_eq!(path, "");
            }
            other => panic!("Expected Smb, got {:?}", other),
        }
    }

    #[test]
    fn detect_unc_server_and_share_only() {
        let proto = detect_protocol("\\\\server\\share");
        match proto {
            Protocol::Smb { server, share, path } => {
                assert_eq!(server, "server");
                assert_eq!(share, "share");
                assert_eq!(path, "");
            }
            other => panic!("Expected Smb, got {:?}", other),
        }
    }
}
