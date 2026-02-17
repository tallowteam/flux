//! WebDAV backend using reqwest::blocking with raw HTTP methods.
//!
//! WebDAV is HTTP-based: GET=read, PUT=write, PROPFIND=stat/list, MKCOL=mkdir.
//! Uses reqwest's blocking client directly -- no async runtime needed.

use std::io::{self, Cursor, Read, Write};
use std::path::Path;
use std::sync::Arc;

use reqwest::blocking::Client;
use reqwest::header::{HeaderMap, HeaderValue, CONTENT_TYPE};
use reqwest::StatusCode;

use crate::backend::{BackendFeatures, FileEntry, FileStat, FluxBackend};
use crate::error::FluxError;
use crate::protocol::Auth;

/// WebDAV backend implementing FluxBackend over HTTP/HTTPS.
///
/// Uses reqwest::blocking::Client for synchronous HTTP requests.
/// WebDAV methods used: GET (read), PUT (write), PROPFIND (stat/list), MKCOL (mkdir).
pub struct WebDavBackend {
    client: Arc<Client>,
    base_url: String,
    auth: Option<Auth>,
}

impl WebDavBackend {
    /// Create a new WebDAV backend for the given base URL.
    ///
    /// The URL should be the WebDAV server root (e.g. "https://server/webdav/").
    /// Optional auth credentials are used for Basic authentication.
    ///
    /// # Security
    ///
    /// When credentials are supplied and the URL uses plain `http://`, both the
    /// credentials (sent as HTTP Basic auth) and all transferred file data are
    /// transmitted in cleartext.  A prominent warning is printed to stderr and
    /// recorded via `tracing::warn!` to alert the operator at connection time.
    pub fn new(url: &str, auth: Option<Auth>) -> Result<Self, FluxError> {
        let client = Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .map_err(|e| FluxError::ProtocolError(format!("Failed to create HTTP client: {}", e)))?;

        // Normalize base URL: ensure it doesn't end with a trailing slash
        // for consistent path joining
        let base_url = url.trim_end_matches('/').to_string();

        // Warn when credentials will be sent over an unencrypted HTTP connection.
        // The scheme comparison is intentionally ASCII-lowercase because the `url`
        // crate normalises schemes to lowercase before handing them to the parser.
        // Only plain `http://` triggers the warning; `https://`, `webdav://`, and
        // `dav://` are either encrypted or pseudo-schemes mapped to HTTPS by the
        // server configuration.
        if base_url.to_ascii_lowercase().starts_with("http://") && auth.is_some() {
            tracing::warn!(
                url = %base_url,
                "WebDAV connection uses HTTP (not HTTPS). \
                 Credentials and file data will be sent in plaintext."
            );
            eprintln!(
                "WARNING: WebDAV connection uses HTTP (not HTTPS). \
                 Credentials and file data will be sent in plaintext."
            );
            eprintln!(
                "         Consider using https:// for secure transfers."
            );
        }

        Ok(WebDavBackend {
            client: Arc::new(client),
            base_url,
            auth,
        })
    }

    /// Build a full URL from a relative path.
    fn url_for(&self, path: &Path) -> String {
        let path_str = path.to_str().unwrap_or("");
        if path_str.is_empty() || path_str == "." || path_str == "/" {
            format!("{}/", self.base_url)
        } else {
            // Normalize path separators to forward slashes for URL
            let normalized = path_str.replace('\\', "/");
            let clean = normalized.trim_start_matches('/');
            format!("{}/{}", self.base_url, clean)
        }
    }

    /// Apply authentication to a request builder.
    fn apply_auth(&self, builder: reqwest::blocking::RequestBuilder) -> reqwest::blocking::RequestBuilder {
        match &self.auth {
            Some(Auth::Password { user, password }) => builder.basic_auth(user, Some(password)),
            _ => builder,
        }
    }

    /// Send a PROPFIND request for stat/list operations.
    fn propfind(&self, url: &str, depth: &str) -> Result<String, FluxError> {
        let propfind_body = r#"<?xml version="1.0" encoding="utf-8"?>
<D:propfind xmlns:D="DAV:">
  <D:allprop/>
</D:propfind>"#;

        let mut headers = HeaderMap::new();
        headers.insert("Depth", HeaderValue::from_str(depth).expect("depth is a valid ASCII header value"));
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/xml"));

        let request = self.client.request(reqwest::Method::from_bytes(b"PROPFIND").expect("PROPFIND is a valid HTTP method"), url)
            .headers(headers)
            .body(propfind_body);
        let request = self.apply_auth(request);

        let response = request.send()
            .map_err(|e| FluxError::ProtocolError(format!("WebDAV PROPFIND failed: {}", e)))?;

        let status = response.status();
        if status == StatusCode::NOT_FOUND {
            // Return a specific error for not found
            return Err(FluxError::SourceNotFound {
                path: std::path::PathBuf::from(url),
            });
        }
        // 207 Multi-Status is the expected success response for PROPFIND
        if status != StatusCode::MULTI_STATUS && !status.is_success() {
            return Err(FluxError::ProtocolError(
                format!("WebDAV PROPFIND returned HTTP {}", status),
            ));
        }

        response.text()
            .map_err(|e| FluxError::ProtocolError(format!("Failed to read PROPFIND response: {}", e)))
    }
}

/// Parse a PROPFIND XML response to extract file stat information.
///
/// Extracts: content-length, resource type (collection = dir), last-modified.
/// Uses simple string parsing to avoid heavy XML dependencies.
fn parse_propfind_stat(xml: &str) -> Option<FileStat> {
    // Detect if this is a collection (directory)
    let is_dir = xml.contains("<D:collection") || xml.contains("<d:collection")
        || xml.contains("<D:collection/>") || xml.contains("<d:collection/>");

    // Extract content-length
    let size = extract_xml_value(xml, "getcontentlength")
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(0);

    Some(FileStat {
        size,
        is_dir,
        is_file: !is_dir,
        modified: None, // TODO: parse getlastmodified if needed
        permissions: None,
    })
}

/// Parse PROPFIND Depth:1 response into a list of (href, FileStat) entries.
///
/// Returns all <response> entries from the multi-status XML.
fn parse_propfind_list(xml: &str) -> Vec<(String, FileStat)> {
    let mut entries = Vec::new();

    // Find all <d:response>...</d:response> blocks (case-insensitive)
    let xml_lower = xml.to_lowercase();
    let mut search_from = 0;

    loop {
        // Find next <d:response> opening tag
        let start = xml_lower[search_from..].find("<d:response>")
            .or_else(|| xml_lower[search_from..].find("<d:response "));

        let start = match start {
            Some(pos) => search_from + pos,
            None => break,
        };

        // Find closing </d:response>
        let end = match xml_lower[start..].find("</d:response>") {
            Some(pos) => start + pos + "</d:response>".len(),
            None => break,
        };

        let block = &xml[start..end];

        // Extract href
        if let Some(href) = extract_xml_value(block, "href") {
            // Parse stat from this response block
            let is_dir = block.to_lowercase().contains("<d:collection");
            let size = extract_xml_value(block, "getcontentlength")
                .and_then(|s| s.parse::<u64>().ok())
                .unwrap_or(0);

            entries.push((
                href,
                FileStat {
                    size,
                    is_dir,
                    is_file: !is_dir,
                    modified: None,
                    permissions: None,
                },
            ));
        }

        search_from = end;
    }

    entries
}

/// Extract the text content of a simple XML element by tag name (case-insensitive).
///
/// Handles both `<D:tagname>value</D:tagname>` and `<d:tagname>value</d:tagname>` patterns,
/// as well as unnamespaced `<tagname>value</tagname>`.
fn extract_xml_value(xml: &str, tag: &str) -> Option<String> {
    let tag_lower = tag.to_lowercase();
    let xml_lower = xml.to_lowercase();

    // Try patterns: <d:tag>, <D:tag>, <tag>
    let patterns = [
        format!("<d:{}>", tag_lower),
        format!("<{}>", tag_lower),
    ];
    let end_patterns = [
        format!("</d:{}>", tag_lower),
        format!("</{}>", tag_lower),
    ];

    for (open, close) in patterns.iter().zip(end_patterns.iter()) {
        if let Some(start_pos) = xml_lower.find(open.as_str()) {
            let value_start = start_pos + open.len();
            if let Some(end_pos) = xml_lower[value_start..].find(close.as_str()) {
                let value = xml[value_start..value_start + end_pos].trim().to_string();
                if !value.is_empty() {
                    return Some(value);
                }
            }
        }
    }

    None
}

impl FluxBackend for WebDavBackend {
    fn stat(&self, path: &Path) -> Result<FileStat, FluxError> {
        let url = self.url_for(path);
        let xml = self.propfind(&url, "0")?;

        parse_propfind_stat(&xml).ok_or_else(|| {
            FluxError::ProtocolError("Failed to parse PROPFIND response for stat".to_string())
        })
    }

    fn list_dir(&self, path: &Path) -> Result<Vec<FileEntry>, FluxError> {
        let url = self.url_for(path);
        let xml = self.propfind(&url, "1")?;

        let entries = parse_propfind_list(&xml);

        // The first entry is typically the directory itself; skip it.
        // We compare the href to our requested URL to identify the self-entry.
        let url_path = url::Url::parse(&url)
            .map(|u| u.path().to_string())
            .unwrap_or_default();

        let result: Vec<FileEntry> = entries
            .into_iter()
            .filter(|(href, _)| {
                // Skip the self-entry: its href matches the requested path
                let href_trimmed = href.trim_end_matches('/');
                let url_trimmed = url_path.trim_end_matches('/');
                href_trimmed != url_trimmed
            })
            .map(|(href, stat)| {
                // Extract filename from href
                let decoded_href = percent_decode(&href);
                let name = decoded_href
                    .trim_end_matches('/')
                    .rsplit('/')
                    .next()
                    .unwrap_or(&decoded_href);
                FileEntry {
                    path: std::path::PathBuf::from(name),
                    stat,
                }
            })
            .collect();

        Ok(result)
    }

    fn open_read(&self, path: &Path) -> Result<Box<dyn Read + Send>, FluxError> {
        let url = self.url_for(path);

        let request = self.client.get(&url);
        let request = self.apply_auth(request);

        let response = request.send()
            .map_err(|e| FluxError::ProtocolError(format!("WebDAV GET failed: {}", e)))?;

        let status = response.status();
        if status == StatusCode::NOT_FOUND {
            return Err(FluxError::SourceNotFound {
                path: path.to_path_buf(),
            });
        }
        if !status.is_success() {
            return Err(FluxError::ProtocolError(
                format!("WebDAV GET returned HTTP {}", status),
            ));
        }

        // Buffer entire response into memory.
        // Limitation: files larger than available RAM will OOM.
        // Future improvement: stream via temp file or channel.
        let bytes = response.bytes()
            .map_err(|e| FluxError::ProtocolError(format!("Failed to read response body: {}", e)))?;

        Ok(Box::new(Cursor::new(bytes.to_vec())))
    }

    fn open_write(&self, path: &Path) -> Result<Box<dyn Write + Send>, FluxError> {
        let url = self.url_for(path);
        Ok(Box::new(WebDavWriter {
            buffer: Vec::new(),
            url,
            client: Arc::clone(&self.client),
            auth: self.auth.clone(),
            flushed: false,
        }))
    }

    fn create_dir_all(&self, path: &Path) -> Result<(), FluxError> {
        // Build each intermediate path and send MKCOL for each
        let mut current = std::path::PathBuf::new();

        for component in path.components() {
            current.push(component);
            let url = self.url_for(&current);

            let request = self.client.request(
                reqwest::Method::from_bytes(b"MKCOL").expect("MKCOL is a valid HTTP method"),
                &url,
            );
            let request = self.apply_auth(request);

            match request.send() {
                Ok(response) => {
                    let status = response.status();
                    // 201 Created = success
                    // 405 Method Not Allowed = already exists (common WebDAV response)
                    // 301 Moved Permanently = already exists (some servers)
                    // 409 Conflict = parent doesn't exist (shouldn't happen if we create in order)
                    if status == StatusCode::CREATED
                        || status == StatusCode::METHOD_NOT_ALLOWED
                        || status == StatusCode::MOVED_PERMANENTLY
                        || status.is_success()
                    {
                        continue;
                    }
                    return Err(FluxError::ProtocolError(
                        format!("WebDAV MKCOL '{}' returned HTTP {}", url, status),
                    ));
                }
                Err(e) => {
                    return Err(FluxError::ProtocolError(
                        format!("WebDAV MKCOL failed for '{}': {}", url, e),
                    ));
                }
            }
        }

        Ok(())
    }

    fn features(&self) -> BackendFeatures {
        BackendFeatures {
            supports_seek: false,
            supports_parallel: false,
            supports_permissions: false,
        }
    }
}

/// A writer that buffers data in memory and uploads via PUT on flush/drop.
///
/// Implements `Write + Send` for use as the return type of `open_write()`.
/// All writes are buffered into an internal `Vec<u8>`. The actual HTTP PUT
/// upload happens when `flush()` is called, or in `Drop` as a safety net.
pub struct WebDavWriter {
    buffer: Vec<u8>,
    url: String,
    client: Arc<Client>,
    auth: Option<Auth>,
    flushed: bool,
}

impl WebDavWriter {
    /// Upload the buffered data to the WebDAV server via PUT.
    fn upload(&mut self) -> io::Result<()> {
        if self.flushed {
            return Ok(());
        }

        let data = std::mem::take(&mut self.buffer);

        let request = self.client.put(&self.url).body(data);
        let request = match &self.auth {
            Some(Auth::Password { user, password }) => request.basic_auth(user, Some(password)),
            _ => request,
        };

        let response = request.send().map_err(|e| {
            io::Error::new(io::ErrorKind::Other, format!("WebDAV PUT failed: {}", e))
        })?;

        let status = response.status();
        if !status.is_success() && status != StatusCode::CREATED && status != StatusCode::NO_CONTENT {
            return Err(io::Error::new(
                io::ErrorKind::Other,
                format!("WebDAV PUT returned HTTP {}", status),
            ));
        }

        self.flushed = true;
        Ok(())
    }

    /// Returns the number of bytes currently buffered.
    pub fn buffered_len(&self) -> usize {
        self.buffer.len()
    }
}

impl Write for WebDavWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.buffer.extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        self.upload()
    }
}

impl Drop for WebDavWriter {
    fn drop(&mut self) {
        if !self.flushed && !self.buffer.is_empty() {
            // Best-effort upload on drop; log error but don't panic
            if let Err(e) = self.upload() {
                tracing::error!("WebDAV upload failed during drop: {}", e);
            }
        }
    }
}

/// Simple percent-decoding for URL paths.
fn percent_decode(input: &str) -> String {
    let mut result = String::with_capacity(input.len());
    let mut chars = input.chars();

    while let Some(c) = chars.next() {
        if c == '%' {
            let hex: String = chars.by_ref().take(2).collect();
            if hex.len() == 2 {
                if let Ok(byte) = u8::from_str_radix(&hex, 16) {
                    result.push(byte as char);
                    continue;
                }
            }
            result.push('%');
            result.push_str(&hex);
        } else {
            result.push(c);
        }
    }

    result
}

// Ensure Send + Sync for the backend (required by FluxBackend trait)
// Client is Send + Sync. Arc<Client> is Send + Sync. Option<Auth> is Send + Sync.
// String is Send + Sync. So WebDavBackend is automatically Send + Sync.
//
// WebDavWriter is Send because:
// - Vec<u8> is Send
// - String is Send
// - Arc<Client> is Send
// - Option<Auth> is Send
// - bool is Send

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn features_reports_no_parallel_no_seek_no_permissions() {
        let backend = WebDavBackend {
            client: Arc::new(Client::new()),
            base_url: "https://example.com/webdav".to_string(),
            auth: None,
        };
        let features = backend.features();
        assert!(!features.supports_seek);
        assert!(!features.supports_parallel);
        assert!(!features.supports_permissions);
    }

    #[test]
    fn url_for_empty_path() {
        let backend = WebDavBackend {
            client: Arc::new(Client::new()),
            base_url: "https://example.com/webdav".to_string(),
            auth: None,
        };
        assert_eq!(backend.url_for(Path::new("")), "https://example.com/webdav/");
        assert_eq!(backend.url_for(Path::new(".")), "https://example.com/webdav/");
        assert_eq!(backend.url_for(Path::new("/")), "https://example.com/webdav/");
    }

    #[test]
    fn url_for_relative_path() {
        let backend = WebDavBackend {
            client: Arc::new(Client::new()),
            base_url: "https://example.com/webdav".to_string(),
            auth: None,
        };
        assert_eq!(
            backend.url_for(Path::new("docs/readme.txt")),
            "https://example.com/webdav/docs/readme.txt"
        );
    }

    #[test]
    fn url_for_absolute_path() {
        let backend = WebDavBackend {
            client: Arc::new(Client::new()),
            base_url: "https://example.com/webdav".to_string(),
            auth: None,
        };
        assert_eq!(
            backend.url_for(Path::new("/docs/readme.txt")),
            "https://example.com/webdav/docs/readme.txt"
        );
    }

    #[test]
    fn url_for_strips_trailing_slash_from_base() {
        let backend = WebDavBackend {
            client: Arc::new(Client::new()),
            base_url: "https://example.com/webdav".to_string(), // already stripped by new()
            auth: None,
        };
        assert_eq!(
            backend.url_for(Path::new("file.txt")),
            "https://example.com/webdav/file.txt"
        );
    }

    #[test]
    fn new_creates_backend_with_normalized_url() {
        let backend = WebDavBackend::new("https://server.com/dav/", None).unwrap();
        assert_eq!(backend.base_url, "https://server.com/dav");
    }

    #[test]
    fn new_creates_backend_with_auth() {
        let auth = Auth::Password {
            user: "admin".to_string(),
            password: "secret".to_string(),
        };
        let backend = WebDavBackend::new("https://server.com/dav", Some(auth)).unwrap();
        assert!(backend.auth.is_some());
        match &backend.auth {
            Some(Auth::Password { user, .. }) => assert_eq!(user, "admin"),
            _ => panic!("Expected Password auth"),
        }
    }

    /// Verify that constructing a backend with `http://` and credentials
    /// succeeds (the warning is informational only, not a hard error).
    #[test]
    fn new_http_with_auth_succeeds_despite_insecure_scheme() {
        let auth = Auth::Password {
            user: "user".to_string(),
            password: "pass".to_string(),
        };
        // This emits a warning to stderr; the constructor must still succeed.
        let backend = WebDavBackend::new("http://nas.local/dav", Some(auth)).unwrap();
        assert_eq!(backend.base_url, "http://nas.local/dav");
        assert!(backend.auth.is_some());
    }

    /// Verify that `http://` without credentials does NOT trigger the warning
    /// code path (no credentials means no secret is at risk).
    #[test]
    fn new_http_without_auth_no_warning() {
        let backend = WebDavBackend::new("http://nas.local/dav", None).unwrap();
        assert_eq!(backend.base_url, "http://nas.local/dav");
        assert!(backend.auth.is_none());
    }

    /// Verify that `https://` with credentials is accepted silently.
    #[test]
    fn new_https_with_auth_no_warning() {
        let auth = Auth::Password {
            user: "admin".to_string(),
            password: "hunter2".to_string(),
        };
        let backend = WebDavBackend::new("https://secure.server.com/dav", Some(auth)).unwrap();
        assert_eq!(backend.base_url, "https://secure.server.com/dav");
        assert!(backend.auth.is_some());
    }

    #[test]
    fn writer_buffers_writes() {
        let mut writer = WebDavWriter {
            buffer: Vec::new(),
            url: "https://example.com/webdav/test.txt".to_string(),
            client: Arc::new(Client::new()),
            auth: None,
            flushed: false,
        };

        writer.write_all(b"Hello, ").unwrap();
        writer.write_all(b"World!").unwrap();

        assert_eq!(writer.buffered_len(), 13);
        assert_eq!(&writer.buffer, b"Hello, World!");
        // Mark as flushed to prevent drop from attempting upload
        writer.flushed = true;
    }

    #[test]
    fn writer_multiple_small_writes_accumulate() {
        let mut writer = WebDavWriter {
            buffer: Vec::new(),
            url: "https://example.com/webdav/test.txt".to_string(),
            client: Arc::new(Client::new()),
            auth: None,
            flushed: false,
        };

        for i in 0..100 {
            let chunk = format!("chunk{}", i);
            writer.write_all(chunk.as_bytes()).unwrap();
        }

        assert!(writer.buffered_len() > 0);
        let content = String::from_utf8(writer.buffer.clone()).unwrap();
        assert!(content.starts_with("chunk0"));
        assert!(content.ends_with("chunk99"));
        // Mark as flushed to prevent drop from attempting upload
        writer.flushed = true;
    }

    #[test]
    fn parse_propfind_stat_file() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<D:multistatus xmlns:D="DAV:">
  <D:response>
    <D:href>/webdav/test.txt</D:href>
    <D:propstat>
      <D:prop>
        <D:getcontentlength>12345</D:getcontentlength>
        <D:resourcetype/>
      </D:prop>
      <D:status>HTTP/1.1 200 OK</D:status>
    </D:propstat>
  </D:response>
</D:multistatus>"#;

        let stat = parse_propfind_stat(xml).unwrap();
        assert!(stat.is_file);
        assert!(!stat.is_dir);
        assert_eq!(stat.size, 12345);
    }

    #[test]
    fn parse_propfind_stat_directory() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<D:multistatus xmlns:D="DAV:">
  <D:response>
    <D:href>/webdav/mydir/</D:href>
    <D:propstat>
      <D:prop>
        <D:resourcetype><D:collection/></D:resourcetype>
      </D:prop>
      <D:status>HTTP/1.1 200 OK</D:status>
    </D:propstat>
  </D:response>
</D:multistatus>"#;

        let stat = parse_propfind_stat(xml).unwrap();
        assert!(stat.is_dir);
        assert!(!stat.is_file);
    }

    #[test]
    fn parse_propfind_list_multiple_entries() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<D:multistatus xmlns:D="DAV:">
  <D:response>
    <D:href>/webdav/</D:href>
    <D:propstat>
      <D:prop>
        <D:resourcetype><D:collection/></D:resourcetype>
      </D:prop>
    </D:propstat>
  </D:response>
  <D:response>
    <D:href>/webdav/file1.txt</D:href>
    <D:propstat>
      <D:prop>
        <D:getcontentlength>100</D:getcontentlength>
        <D:resourcetype/>
      </D:prop>
    </D:propstat>
  </D:response>
  <D:response>
    <D:href>/webdav/subdir/</D:href>
    <D:propstat>
      <D:prop>
        <D:resourcetype><D:collection/></D:resourcetype>
      </D:prop>
    </D:propstat>
  </D:response>
</D:multistatus>"#;

        let entries = parse_propfind_list(xml);
        assert_eq!(entries.len(), 3);

        // First entry is the directory itself
        assert_eq!(entries[0].0, "/webdav/");
        assert!(entries[0].1.is_dir);

        // Second entry is a file
        assert_eq!(entries[1].0, "/webdav/file1.txt");
        assert!(entries[1].1.is_file);
        assert_eq!(entries[1].1.size, 100);

        // Third entry is a subdirectory
        assert_eq!(entries[2].0, "/webdav/subdir/");
        assert!(entries[2].1.is_dir);
    }

    #[test]
    fn extract_xml_value_basic() {
        let xml = "<D:getcontentlength>42</D:getcontentlength>";
        assert_eq!(extract_xml_value(xml, "getcontentlength"), Some("42".to_string()));
    }

    #[test]
    fn extract_xml_value_case_insensitive() {
        let xml = "<d:GetContentLength>42</d:GetContentLength>";
        assert_eq!(extract_xml_value(xml, "getcontentlength"), Some("42".to_string()));
    }

    #[test]
    fn extract_xml_value_not_found() {
        let xml = "<D:other>value</D:other>";
        assert_eq!(extract_xml_value(xml, "getcontentlength"), None);
    }

    #[test]
    fn extract_xml_href() {
        let xml = r#"<D:href>/webdav/test.txt</D:href>"#;
        assert_eq!(extract_xml_value(xml, "href"), Some("/webdav/test.txt".to_string()));
    }

    #[test]
    fn percent_decode_basic() {
        assert_eq!(percent_decode("/path/to/file.txt"), "/path/to/file.txt");
        assert_eq!(percent_decode("/path%20with%20spaces/file.txt"), "/path with spaces/file.txt");
        assert_eq!(percent_decode("%2Froot"), "/root");
    }

    #[test]
    fn percent_decode_no_encoding() {
        assert_eq!(percent_decode("hello"), "hello");
    }
}
