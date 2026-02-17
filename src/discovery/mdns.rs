use crate::discovery::service::{DiscoveredDevice, FluxService, SERVICE_TYPE};
use crate::error::FluxError;
use gethostname::gethostname;
use mdns_sd::{ServiceDaemon, ServiceEvent, ServiceInfo};
use std::collections::HashMap;
use std::time::{Duration, Instant};

/// Register this device as a Flux service on the local network via mDNS.
///
/// Creates a `ServiceDaemon` that advertises a `_flux._tcp.local.` service
/// with the given device name and port. The daemon runs in a background thread
/// managed by `mdns-sd` -- the caller must keep the returned daemon alive
/// to maintain the registration.
///
/// TXT properties advertised:
/// - `version`: the Flux package version (from Cargo.toml)
/// - `pubkey`: base64-encoded public key (optional, for TOFU)
///
/// # Arguments
/// - `service`: The FluxService describing this device
/// - `public_key`: Optional base64-encoded public key to advertise
///
/// # Returns
/// The `ServiceDaemon` handle. Drop it to unregister.
pub fn register_flux_service(
    service: &FluxService,
    public_key: Option<&str>,
) -> Result<ServiceDaemon, FluxError> {
    let mdns = ServiceDaemon::new()
        .map_err(|e| FluxError::DiscoveryError(format!("Failed to create mDNS daemon: {}", e)))?;

    let hostname = gethostname().to_string_lossy().to_string();
    let host_label = format!("{}.local.", hostname);

    // Build TXT properties
    let mut properties: Vec<(&str, &str)> = vec![
        ("version", env!("CARGO_PKG_VERSION")),
    ];

    // Temporarily hold the public_key to satisfy borrow checker
    if let Some(pk) = public_key {
        properties.push(("pubkey", pk));
    }

    let service_info = ServiceInfo::new(
        SERVICE_TYPE,
        &service.device_name,
        &host_label,
        "",  // empty string = auto-detect IP addresses
        service.port,
        properties.as_slice(),
    )
    .map_err(|e| FluxError::DiscoveryError(format!("Invalid service info: {}", e)))?
    .enable_addr_auto();

    mdns.register(service_info)
        .map_err(|e| FluxError::DiscoveryError(format!("Failed to register service: {}", e)))?;

    Ok(mdns)
}

/// Discover Flux devices on the local network via mDNS.
///
/// Browses for `_flux._tcp.local.` services for the given timeout period,
/// collecting all resolved devices. Uses synchronous channel operations
/// (mdns-sd runs its own internal thread).
///
/// Devices are deduplicated by name (first occurrence wins).
///
/// # Arguments
/// - `timeout_secs`: How long to browse for devices (in seconds)
///
/// # Returns
/// A vector of discovered devices, possibly empty if none found.
pub fn discover_flux_devices(timeout_secs: u64) -> Result<Vec<DiscoveredDevice>, FluxError> {
    let mdns = ServiceDaemon::new()
        .map_err(|e| FluxError::DiscoveryError(format!("Failed to create mDNS daemon: {}", e)))?;

    let receiver = mdns
        .browse(SERVICE_TYPE)
        .map_err(|e| FluxError::DiscoveryError(format!("Failed to browse: {}", e)))?;

    let mut seen: HashMap<String, DiscoveredDevice> = HashMap::new();
    let deadline = Instant::now() + Duration::from_secs(timeout_secs);

    while Instant::now() < deadline {
        match receiver.recv_timeout(Duration::from_millis(500)) {
            Ok(ServiceEvent::ServiceResolved(info)) => {
                // Extract the instance name from the fullname
                // fullname format: "instance-name._flux._tcp.local."
                let instance_name = extract_instance_name(&info.fullname);

                // Get the first IP address (prefer IPv4)
                let addr = info
                    .addresses
                    .iter()
                    .find(|a| a.is_ipv4())
                    .or_else(|| info.addresses.iter().next());

                if let Some(scoped_ip) = addr {
                    let host = scoped_ip.to_ip_addr().to_string();

                    // Extract TXT properties
                    let version = info
                        .txt_properties
                        .get("version")
                        .map(|p| p.val_str().to_string());
                    let public_key = info
                        .txt_properties
                        .get("pubkey")
                        .map(|p| p.val_str().to_string());

                    // Deduplicate by instance name (first seen wins)
                    if !seen.contains_key(&instance_name) {
                        seen.insert(
                            instance_name.clone(),
                            DiscoveredDevice {
                                name: instance_name,
                                host,
                                port: info.port,
                                version,
                                public_key,
                            },
                        );
                    }
                }
            }
            Ok(_) => {
                // SearchStarted, ServiceFound (unresolved), ServiceRemoved, etc.
            }
            Err(_) => {
                // recv_timeout expired, continue until deadline
            }
        }
    }

    // Shut down the daemon cleanly
    mdns.shutdown().ok();

    Ok(seen.into_values().collect())
}

/// Extract the instance name from an mDNS fullname.
///
/// The fullname format is: `instance-name._flux._tcp.local.`
/// This function strips the service type suffix to get just the instance name.
fn extract_instance_name(fullname: &str) -> String {
    // The fullname is "instance._flux._tcp.local."
    // We want just "instance"
    if let Some(idx) = fullname.find(SERVICE_TYPE) {
        let prefix = &fullname[..idx];
        // Remove trailing dot from instance name
        prefix.trim_end_matches('.').to_string()
    } else {
        // Fallback: just use the whole name
        fullname.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_instance_name_basic() {
        let fullname = format!("my-laptop.{}", SERVICE_TYPE);
        assert_eq!(extract_instance_name(&fullname), "my-laptop");
    }

    #[test]
    fn extract_instance_name_with_dots() {
        // Instance names can have escaped dots
        let fullname = format!("my\\.laptop.{}", SERVICE_TYPE);
        assert_eq!(extract_instance_name(&fullname), "my\\.laptop");
    }

    #[test]
    fn extract_instance_name_fallback() {
        assert_eq!(
            extract_instance_name("something-unknown"),
            "something-unknown"
        );
    }

    // Note: Actual mDNS network tests require a network interface and cannot
    // reliably run in CI environments. The following tests are marked #[ignore]
    // and should be run manually on a machine with network access.

    #[test]
    #[ignore]
    fn test_register_and_discover() {
        use crate::discovery::service::DEFAULT_PORT;

        let service = FluxService::new(Some("test-flux-device".to_string()), DEFAULT_PORT);
        let daemon = register_flux_service(&service, None).unwrap();

        // Give mDNS time to propagate
        std::thread::sleep(Duration::from_secs(2));

        let devices = discover_flux_devices(3).unwrap();

        // We should find at least our own service
        let found = devices.iter().any(|d| d.name == "test-flux-device");
        assert!(found, "Should discover our own registered service");

        daemon.shutdown().ok();
    }
}
