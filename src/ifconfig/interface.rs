use regex::Regex;
use std::error::Error;
use std::fmt;
/// interface.rs
/// handle interface-related logic for the ifconfig utility on Redox OS.
use std::fs;
use std::net::IpAddr;
use std::path::Path;

/// Custom error type for interface operations
#[derive(Debug)]
pub enum InterfaceError {
    NotFound(String),
    ReadError(String),
    InvalidMacAddress(String),
    InvalidIpAddress(String),
    // Additional error cases can be added here
}

/// Implement Display trait for InterfaceError for user-friendly error messages
impl fmt::Display for InterfaceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            InterfaceError::NotFound(msg) => write!(f, "Interface not found: {}", msg),
            InterfaceError::ReadError(msg) => write!(f, "Read error: {}", msg),
            InterfaceError::InvalidMacAddress(addr) => write!(f, "Invalid MAC address: {}", addr),
            InterfaceError::InvalidIpAddress(addr) => write!(f, "Invalid IP address: {}", addr),
        }
    }
}

/// Implement the Error trait for InterfaceError
impl Error for InterfaceError {}

/// Structure to represent a network interface
pub struct NetworkInterface {
    pub name: String,
    pub mac_address: String,
    pub ip_address: String,
    pub netmask: String,
    // Additional fields can be added here
}

/// Implement methods for NetworkInterface
impl NetworkInterface {
    pub fn new(iface: &str) -> Result<Self, InterfaceError> {
        // Validate the interface name
        if iface.is_empty() {
            return Err(InterfaceError::NotFound(
                "Interface name is empty".to_string(),
            ));
        }

        // Get IP address and netmask from addr/list
        let addr_data = get_iface_cfg_value(iface, "addr/list")?;
        let (ip_address, netmask) = parse_ip_and_netmask(&addr_data)?;

        // Placeholder for MAC address (not available in this structure)
        let mac_address = "00:00:00:00:00:00".to_string();

        // Create the NetworkInterface instance
        Ok(NetworkInterface {
            name: iface.to_string(),
            mac_address,
            ip_address,
            netmask,
        })
    }
}

/// Implement Display trait for NetworkInterface to format output
impl fmt::Display for NetworkInterface {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "{}:", self.name)?;
        writeln!(f, "    MAC Address: {}", self.mac_address)?;
        writeln!(f, "    IP Address: {}", self.ip_address)?;
        writeln!(f, "    Netmask: {}", self.netmask)
    }
}

/// Parses IP address and netmask from a string
fn parse_ip_and_netmask(addr_data: &str) -> Result<(String, String), InterfaceError> {
    // Split the address and netmask (e.g., "10.0.2.15/24")
    let parts: Vec<&str> = addr_data.split('/').collect();
    if parts.len() != 2 {
        return Err(InterfaceError::InvalidIpAddress(addr_data.to_string()));
    }
    let ip_address = parts[0].to_string();
    let netmask = parts[1].to_string();
    Ok((ip_address, netmask))
}

/// Reads the value of a configuration file for a given interface
fn get_iface_cfg_value(iface: &str, cfg: &str) -> Result<String, InterfaceError> {
    let base_path = Path::new("/scheme/netcfg/ifaces").join(iface).join(cfg);

    if !base_path.exists() {
        return Err(InterfaceError::NotFound(format!(
            "Path does not exist: {}",
            base_path.display()
        )));
    }

    fs::read_to_string(&base_path)
        .map(|s| s.trim().to_string())
        .map_err(|e| {
            InterfaceError::ReadError(format!("Failed to read {}: {}", base_path.display(), e))
        })
}

/// Lists all available network interfaces
pub fn list_all_interfaces() -> Result<Vec<NetworkInterface>, InterfaceError> {
    let path = Path::new("/scheme/netcfg/ifaces");
    if !path.exists() {
        return Ok(vec![]); // Return an empty list if no interfaces directory exists
    }

    let entries = fs::read_dir(path)
        .map_err(|e| InterfaceError::ReadError(format!("Failed to read interfaces: {}", e)))?;

    let mut interfaces = Vec::new();
    for entry in entries {
        let entry =
            entry.map_err(|e| InterfaceError::ReadError(format!("Failed to read entry: {}", e)))?;
        if let Some(iface_name) = entry.file_name().to_str() {
            // Try to create a NetworkInterface instance
            match NetworkInterface::new(iface_name) {
                Ok(interface) => interfaces.push(interface),
                Err(e) => eprintln!("Skipping interface '{}': {}", iface_name, e),
            }
        }
    }
    Ok(interfaces)
}

/// Validates the format of a MAC address
fn validate_mac_address(mac: &str) -> Result<(), InterfaceError> {
    // Regular expression for MAC address validation
    let mac_regex = Regex::new(r"^([0-9A-Fa-f]{2}:){5}[0-9A-Fa-f]{2}$")
        .map_err(|e| InterfaceError::InvalidMacAddress(format!("Regex error: {}", e)))?;
    if mac_regex.is_match(mac) {
        Ok(())
    } else {
        Err(InterfaceError::InvalidMacAddress(mac.to_string()))
    }
}

/// Validates and parses an IP address
fn validate_ip_address(ip: &str) -> Result<IpAddr, InterfaceError> {
    ip.parse::<IpAddr>()
        .map_err(|_| InterfaceError::InvalidIpAddress(ip.to_string()))
}

/// Configures a network interface (placeholder function)
#[allow(dead_code)]
pub fn configure_interface(_iface: &str, mac: &str, ip: &str) -> Result<(), InterfaceError> {
    // Validate the MAC address
    validate_mac_address(mac)?;

    // Validate the IP address
    let _parsed_ip = validate_ip_address(ip)?;

    // Proceed with configuration (not implemented)
    // ...

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_mac_address() {
        assert!(validate_mac_address("00:1A:2B:3C:4D:5E").is_ok());
        assert!(validate_mac_address("00:1a:2b:3c:4d:5e").is_ok());
        assert!(validate_mac_address("00-1A-2B-3C-4D-5E").is_err());
        assert!(validate_mac_address("001A:2B:3C:4D:5E").is_err());
    }

    #[test]
    fn test_validate_ip_address() {
        assert!(validate_ip_address("192.168.1.1").is_ok());
        assert!(validate_ip_address("255.255.255.255").is_ok());
        assert!(validate_ip_address("999.999.999.999").is_err());
        assert!(validate_ip_address("::1").is_ok()); // IPv6 loopback
    }
}
