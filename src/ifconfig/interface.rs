/// interface.rs
use std::error::Error;
use std::fmt;
use std::fs::{self, File};
use std::io::{self, Read};
use std::path::Path;

#[derive(Debug)]
pub struct NetworkInterface {
    pub name: String,
    pub mac: String,
    pub ips: Vec<String>,
    pub status: String,
}

impl NetworkInterface {
    pub fn new(name: &str) -> Result<Self, Box<dyn Error>> {
        //println!("DEBUG: Initializing interface {}", name); // Debugging
        let iface_path = format!("/scheme/netcfg/ifaces/{}", name);

        if !Path::new(&iface_path).exists() {
            return Err(format!("Interface '{}' not found at {}", name, iface_path).into());
        }

        // Read MAC address
        let mac = get_iface_cfg_value(name, "mac")?;
        //println!("DEBUG: MAC for {} is {}", name, mac); // Debugging

        // Read IP addresses
        let ip_list = get_iface_cfg_value(name, "addr/list")?;
        //println!("DEBUG: IP list for {} is {}", name, ip_list); // Debugging
        let ips = ip_list.lines().map(|line| line.to_string()).collect();

        // Read interface status
        let status = get_iface_cfg_value(name, "status").unwrap_or_else(|_| "UNKNOWN".to_string());
        //println!("DEBUG: Status for {} is {}", name, status); // Debugging

        Ok(NetworkInterface {
            name: name.to_string(),
            mac,
            ips,
            status,
        })
    }
}

impl fmt::Display for NetworkInterface {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "Interface: {}", self.name)?;
        writeln!(f, "  MAC Address: {}", self.mac)?;
        for ip in &self.ips {
            writeln!(f, "  IP Address: {}", ip)?;
        }
        write!(f, "  Status: {}", self.status)
    }
}

fn get_iface_cfg_value(iface: &str, cfg: &str) -> Result<String, Box<dyn Error>> {
    let path = format!("/scheme/netcfg/ifaces/{}/{}", iface, cfg);
    //println!("DEBUG: Attempting to read {}", path); // Debugging
    let mut file = File::open(&path).map_err(|e| format!("Failed to open {}: {}", path, e))?;
    let mut result = String::new();
    file.read_to_string(&mut result)
        .map_err(|e| format!("Failed to read from {}: {}", path, e))?;
    Ok(result.trim().to_string())
}

pub fn list_all_interfaces() -> Result<Vec<NetworkInterface>, Box<dyn Error>> {
    let ifaces_path = "/scheme/netcfg/ifaces";
    //println!("DEBUG: Listing all interfaces in {}", ifaces_path); // Debugging
    let entries = fs::read_dir(ifaces_path)
        .map_err(|e| format!("Failed to read directory {}: {}", ifaces_path, e))?;

    let mut interfaces = Vec::new();

    for entry in entries {
        let entry = entry?;
        let iface_name = entry.file_name().into_string().unwrap_or_default();
        //println!("DEBUG: Found interface {}", iface_name); // Debugging

        match NetworkInterface::new(&iface_name) {
            Ok(interface) => {
                //println!("DEBUG: Successfully initialized interface '{}'", iface_name);
                interfaces.push(interface);
            }
            Err(e) => {
                println!(
                    "ERROR: Failed to initialize interface '{}': {}",
                    iface_name, e
                );
            }
        }
    }

    Ok(interfaces)
}
