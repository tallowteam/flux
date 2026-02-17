use gethostname::gethostname;

/// The mDNS service type for Flux device discovery.
/// Must follow RFC 6763: _service._tcp.local. with trailing dot.
pub const SERVICE_TYPE: &str = "_flux._tcp.local.";

/// Default TCP port for the Flux transfer protocol.
pub const DEFAULT_PORT: u16 = 9741;

/// Maximum DNS label length per RFC 1035.
const MAX_DNS_LABEL_LEN: usize = 63;

/// A discovered Flux device on the LAN.
#[derive(Debug, Clone)]
pub struct DiscoveredDevice {
    /// Friendly instance name (e.g. "alice-laptop")
    pub name: String,
    /// IP address as a string
    pub host: String,
    /// TCP port for the transfer protocol
    pub port: u16,
    /// Flux version from TXT record, if available
    pub version: Option<String>,
    /// Base64-encoded public key from TXT record, for TOFU verification
    pub public_key: Option<String>,
}

/// Represents this device's Flux service identity for mDNS registration.
#[derive(Debug, Clone)]
pub struct FluxService {
    /// The friendly device name advertised via mDNS
    pub device_name: String,
    /// The port to listen on
    pub port: u16,
}

impl FluxService {
    /// Create a new FluxService.
    ///
    /// If `device_name` is None, the system hostname is used as the friendly
    /// name. The name is sanitized: non-alphanumeric characters (except hyphens)
    /// are replaced with hyphens, and the result is truncated to 63 characters
    /// (the DNS label limit per RFC 1035).
    pub fn new(device_name: Option<String>, port: u16) -> Self {
        let raw_name = device_name.unwrap_or_else(|| {
            gethostname().to_string_lossy().to_string()
        });

        let sanitized = sanitize_device_name(&raw_name);

        FluxService {
            device_name: sanitized,
            port,
        }
    }
}

/// Sanitize a device name for use as a DNS label.
///
/// - Replace non-alphanumeric characters (except hyphen) with hyphens
/// - Collapse consecutive hyphens
/// - Strip leading/trailing hyphens
/// - Truncate to 63 characters (DNS label limit)
/// - If result is empty, use "flux-device"
fn sanitize_device_name(name: &str) -> String {
    let sanitized: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' {
                c
            } else {
                '-'
            }
        })
        .collect();

    // Collapse consecutive hyphens
    let mut result = String::with_capacity(sanitized.len());
    let mut prev_hyphen = false;
    for c in sanitized.chars() {
        if c == '-' {
            if !prev_hyphen {
                result.push(c);
            }
            prev_hyphen = true;
        } else {
            result.push(c);
            prev_hyphen = false;
        }
    }

    // Strip leading/trailing hyphens
    let trimmed = result.trim_matches('-');

    // Truncate to DNS label limit
    let truncated = if trimmed.len() > MAX_DNS_LABEL_LEN {
        // Truncate at char boundary (safe since we only have ASCII at this point)
        &trimmed[..MAX_DNS_LABEL_LEN]
    } else {
        trimmed
    };

    // Strip trailing hyphen after truncation
    let final_name = truncated.trim_end_matches('-');

    if final_name.is_empty() {
        "flux-device".to_string()
    } else {
        final_name.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn service_type_format() {
        assert!(SERVICE_TYPE.starts_with('_'));
        assert!(SERVICE_TYPE.contains("._tcp."));
        assert!(SERVICE_TYPE.ends_with(".local."));
        assert_eq!(SERVICE_TYPE, "_flux._tcp.local.");
    }

    #[test]
    fn default_port_value() {
        assert_eq!(DEFAULT_PORT, 9741);
    }

    #[test]
    fn flux_service_new_with_provided_name() {
        let svc = FluxService::new(Some("my-laptop".to_string()), 9741);
        assert_eq!(svc.device_name, "my-laptop");
        assert_eq!(svc.port, 9741);
    }

    #[test]
    fn flux_service_new_with_none_uses_hostname() {
        let svc = FluxService::new(None, DEFAULT_PORT);
        // Should not be empty -- gethostname always returns something
        assert!(!svc.device_name.is_empty());
        assert_eq!(svc.port, DEFAULT_PORT);
    }

    #[test]
    fn sanitize_replaces_special_chars() {
        assert_eq!(sanitize_device_name("hello world"), "hello-world");
        assert_eq!(sanitize_device_name("my.laptop.local"), "my-laptop-local");
        assert_eq!(sanitize_device_name("test@host#1"), "test-host-1");
    }

    #[test]
    fn sanitize_collapses_consecutive_hyphens() {
        assert_eq!(sanitize_device_name("a---b"), "a-b");
        assert_eq!(sanitize_device_name("hello   world"), "hello-world");
    }

    #[test]
    fn sanitize_strips_leading_trailing_hyphens() {
        assert_eq!(sanitize_device_name("-hello-"), "hello");
        assert_eq!(sanitize_device_name("---test---"), "test");
        assert_eq!(sanitize_device_name(".leading.dot."), "leading-dot");
    }

    #[test]
    fn sanitize_truncates_long_names() {
        let long_name = "a".repeat(100);
        let result = sanitize_device_name(&long_name);
        assert!(result.len() <= MAX_DNS_LABEL_LEN);
        assert_eq!(result.len(), MAX_DNS_LABEL_LEN);
    }

    #[test]
    fn sanitize_empty_name_falls_back() {
        assert_eq!(sanitize_device_name(""), "flux-device");
        assert_eq!(sanitize_device_name("..."), "flux-device");
        assert_eq!(sanitize_device_name("---"), "flux-device");
    }

    #[test]
    fn sanitize_preserves_alphanumeric_and_hyphens() {
        assert_eq!(sanitize_device_name("abc-123"), "abc-123");
        assert_eq!(sanitize_device_name("DESKTOP-ABC"), "DESKTOP-ABC");
    }

    #[test]
    fn flux_service_custom_port() {
        let svc = FluxService::new(Some("test".to_string()), 8080);
        assert_eq!(svc.port, 8080);
    }
}
