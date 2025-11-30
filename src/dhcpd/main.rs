use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::net::{SocketAddr, UdpSocket};
use std::time::Duration;
use std::{env, process, time};

use dhcp::Dhcp;

mod dhcp;

macro_rules! try_fmt {
    ($e:expr, $m:expr) => {
        match $e {
            Ok(ok) => ok,
            Err(err) => return Err(format!("{}: {}", $m, err)),
        }
    };
}

fn get_cfg_value(path: &str) -> Result<String, String> {
    let path = format!("/scheme/netcfg/{path}");
    let mut file = File::open(&path).map_err(|_| format!("Can't open {path}"))?;
    let mut result = String::new();
    file.read_to_string(&mut result)
        .map_err(|_| format!("Can't read {path}"))?;
    Ok(result)
}

fn get_iface_cfg_value(iface: &str, cfg: &str) -> Result<String, String> {
    let path = format!("ifaces/{iface}/{cfg}");
    get_cfg_value(&path)
}

fn set_cfg_value(path: &str, value: &str) -> Result<(), String> {
    let path = format!("/scheme/netcfg/{path}");
    let mut file = OpenOptions::new()
        .read(false)
        .write(true)
        .create(false)
        .open(&path)
        .map_err(|_| format!("Can't open {path}"))?;
    file.write(value.as_bytes())
        .map(|_| ())
        .map_err(|_| format!("Can't write {value} to {path}"))?;
    file.sync_data()
        .map_err(|_| format!("Can't commit {value} to {path}"))
}

fn set_iface_cfg_value(iface: &str, cfg: &str, value: &str) -> Result<(), String> {
    let path = format!("ifaces/{iface}/{cfg}");
    set_cfg_value(&path, value)
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Default)]
struct MacAddr {
    bytes: [u8; 6],
}

impl MacAddr {
    fn from_str(string: &str) -> Self {
        MacAddr::try_parse_with_delimeter(string, ':')
            .or_else(|| MacAddr::try_parse_with_delimeter(string, '-'))
            .unwrap_or_default()
    }

    fn try_parse_with_delimeter(string: &str, delimeter: char) -> Option<MacAddr> {
        let mut addr = MacAddr::default();
        let mut segments = 0;

        for part in string.split(delimeter) {
            if segments >= addr.bytes.len() {
                return None;
            }
            addr.bytes[segments] = match u8::from_str_radix(part, 16) {
                Ok(b) => b,
                _ => return None,
            };
            segments += 1;
        }

        if segments == addr.bytes.len() {
            Some(addr)
        } else {
            None
        }
    }

    fn to_string(&self) -> String {
        format!(
            "{:>02X}-{:>02X}-{:>02X}-{:>02X}-{:>02X}-{:>02X}",
            self.bytes[0],
            self.bytes[1],
            self.bytes[2],
            self.bytes[3],
            self.bytes[4],
            self.bytes[5]
        )
    }
}

fn dhcp(iface: &str, verbose: bool) -> Result<(), String> {
    let current_mac = MacAddr::from_str(get_iface_cfg_value(iface, "mac")?.trim());

    let current_ip = get_iface_cfg_value(iface, "addr/list")?
        .lines()
        .next()
        .map(|l| l.to_owned())
        .unwrap_or("0.0.0.0".to_string());

    if verbose {
        println!(
            "DHCP: MAC: {} Current IP: {}",
            current_mac.to_string(),
            current_ip.trim()
        );
    }

    let tid = try_fmt!(
        time::SystemTime::now().duration_since(time::UNIX_EPOCH),
        "failed to get time"
    )
    .subsec_nanos();

    let socket = try_fmt!(UdpSocket::bind(("0.0.0.0", 68)), "failed to bind udp");
    try_fmt!(
        socket.connect(SocketAddr::from(([255, 255, 255, 255], 67))),
        "failed to connect udp"
    );
    try_fmt!(
        socket.set_read_timeout(Some(Duration::new(30, 0))),
        "failed to set read timeout"
    );
    try_fmt!(
        socket.set_write_timeout(Some(Duration::new(30, 0))),
        "failed to set write timeout"
    );

    {
        let mut discover = Dhcp {
            op: 1,
            htype: 1,
            hlen: 6,
            hops: 0,
            tid,
            secs: 0,
            flags: 0x8000u16.to_be(),
            ciaddr: [0, 0, 0, 0],
            yiaddr: [0, 0, 0, 0],
            siaddr: [0, 0, 0, 0],
            giaddr: [0, 0, 0, 0],
            chaddr: [
                current_mac.bytes[0],
                current_mac.bytes[1],
                current_mac.bytes[2],
                current_mac.bytes[3],
                current_mac.bytes[4],
                current_mac.bytes[5],
                0x00,
                0x00,
                0x00,
                0x00,
                0x00,
                0x00,
                0x00,
                0x00,
                0x00,
                0x00,
            ],
            sname: [0; 64],
            file: [0; 128],
            magic: 0x63825363u32.to_be(),
            options: [0; 308],
        };

        for (s, d) in [
            // DHCP Message Type (Discover)
            53, 1, 1, // End
            255,
        ]
        .iter()
        .zip(discover.options.iter_mut())
        {
            *d = *s;
        }

        let discover_data = unsafe {
            std::slice::from_raw_parts(
                (&discover as *const Dhcp) as *const u8,
                std::mem::size_of::<Dhcp>(),
            )
        };

        let _sent = try_fmt!(socket.send(discover_data), "failed to send discover");

        if verbose {
            println!("DHCP: Sent Discover");
        }
    }

    let mut offer_data = [0; 65536];
    try_fmt!(socket.recv(&mut offer_data), "failed to receive offer");
    let offer = unsafe { &*(offer_data.as_ptr() as *const Dhcp) };
    if verbose {
        println!(
            "DHCP: Offer IP: {:?}, Server IP: {:?}",
            offer.yiaddr, offer.siaddr
        );
    }

    let mut subnet_option = None;
    let mut router_option = None;
    let mut dns_option = None;
    let mut server_id_option = None;
    {
        let mut options = offer.options.iter();
        while let Some(option) = options.next() {
            match *option {
                0 => (),
                255 => break,
                _ => {
                    if let Some(len) = options.next() {
                        if *len as usize <= options.as_slice().len() {
                            let data = &options.as_slice()[..*len as usize];
                            for _data_i in 0..*len {
                                options.next();
                            }
                            match *option {
                                1 => {
                                    if verbose {
                                        println!("DHCP: Subnet Mask: {data:?}");
                                    }
                                    if data.len() == 4 && subnet_option.is_none() {
                                        subnet_option = Some(Vec::from(data));
                                    }
                                }
                                3 => {
                                    if verbose {
                                        println!("DHCP: Router: {data:?}");
                                    }
                                    if data.len() == 4 && router_option.is_none() {
                                        router_option = Some(Vec::from(data));
                                    }
                                }
                                6 => {
                                    if verbose {
                                        println!("DHCP: Domain Name Server: {data:?}");
                                    }
                                    if data.len() == 4 && dns_option.is_none() {
                                        dns_option = Some(Vec::from(data));
                                    }
                                }
                                51 => {
                                    if verbose {
                                        println!("DHCP: Lease Time: {data:?}");
                                    }
                                }
                                53 => {
                                    if verbose {
                                        println!("DHCP: Message Type: {data:?}");
                                    }
                                }
                                54 => {
                                    if verbose {
                                        println!("DHCP: Server ID: {data:?}");
                                    }
                                    if data.len() == 4 {
                                        // Store the server ID
                                        server_id_option =
                                            Some([data[0], data[1], data[2], data[3]]);
                                    }
                                }
                                _ => {
                                    if verbose {
                                        println!("DHCP: {option}: {data:?}");
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        let mask_len = if let Some(subnet) = subnet_option {
            let mut subnet: u32 = (subnet[0] as u32) << 24
                | (subnet[1] as u32) << 16
                | (subnet[2] as u32) << 8
                | subnet[3] as u32;
            subnet = !subnet;
            subnet.leading_zeros()
        } else {
            0
        };

        let new_ips = format!(
            "{}.{}.{}.{}/{}\n",
            offer.yiaddr[0], offer.yiaddr[1], offer.yiaddr[2], offer.yiaddr[3], mask_len
        );
        try_fmt!(
            set_iface_cfg_value(iface, "addr/set", &new_ips),
            "failed to set ip"
        );

        if verbose {
            let new_ip = try_fmt!(get_iface_cfg_value(iface, "addr/list"), "failed to get ip");
            println!("DHCP: New IP: {}", new_ip.trim());
        }

        if let Some(router) = router_option {
            let default_route = format!(
                "default via {}.{}.{}.{}",
                router[0], router[1], router[2], router[3]
            );

            try_fmt!(
                set_cfg_value("route/add", &default_route),
                "failed to set default route"
            );

            if verbose {
                let new_router = try_fmt!(get_cfg_value("route/list"), "failed to get ip router");
                println!("DHCP: New Router: {}", new_router.trim());
            }
        }

        if let Some(mut dns) = dns_option {
            if dns[0] == 127 {
                let quad9 = [9, 9, 9, 9].to_vec();
                if verbose {
                    println!("DHCP: Received sarcastic DNS suggestion {}.{}.{}.{}, using {}.{}.{}.{} instead",
                            dns[0], dns[1], dns[2], dns[3], quad9[0], quad9[1], quad9[2], quad9[3]);
                }
                dns = quad9;
            }

            let nameserver = format!("{}.{}.{}.{}", dns[0], dns[1], dns[2], dns[3]);

            try_fmt!(
                set_cfg_value("resolv/nameserver", &nameserver),
                "failed to set name server"
            );

            if verbose {
                let new_dns = try_fmt!(get_cfg_value("resolv/nameserver"), "failed to get dns");
                println!("DHCP: New DNS: {}", new_dns.trim());
            }
        }
    }

    {
        let mut request = Dhcp {
            op: 1,
            htype: 1,
            hlen: 6,
            hops: 0,
            tid,
            secs: 0,
            flags: 0,
            ciaddr: [0; 4],
            yiaddr: [0; 4],
            siaddr: [0; 4],
            giaddr: [0; 4],
            chaddr: [
                current_mac.bytes[0],
                current_mac.bytes[1],
                current_mac.bytes[2],
                current_mac.bytes[3],
                current_mac.bytes[4],
                current_mac.bytes[5],
                0x00,
                0x00,
                0x00,
                0x00,
                0x00,
                0x00,
                0x00,
                0x00,
                0x00,
                0x00,
            ],
            sname: [0; 64],
            file: [0; 128],
            magic: 0x63825363u32.to_be(),
            options: [0; 308],
        };

        // If the server_id_option was None, use "0.0.0.0"
        let server_id = server_id_option.unwrap_or([0, 0, 0, 0]);

        for (s, d) in [
            // DHCP Message Type (Request)
            53,
            1,
            3,
            // Requested IP Address
            50,
            4,
            offer.yiaddr[0],
            offer.yiaddr[1],
            offer.yiaddr[2],
            offer.yiaddr[3],
            // Server Identifier - use Option 54 from the Offer
            54,
            4,
            server_id[0],
            server_id[1],
            server_id[2],
            server_id[3],
            // End
            255,
        ]
        .iter()
        .zip(request.options.iter_mut())
        {
            *d = *s;
        }

        let request_data = unsafe {
            std::slice::from_raw_parts(
                (&request as *const Dhcp) as *const u8,
                std::mem::size_of::<Dhcp>(),
            )
        };

        let _sent = try_fmt!(socket.send(request_data), "failed to send request");

        if verbose {
            println!("DHCP: Sent Request");
        }
    }

    {
        let mut ack_data = [0; 65536];
        try_fmt!(socket.recv(&mut ack_data), "failed to receive ack");
        let ack = unsafe { &*(ack_data.as_ptr() as *const Dhcp) };
        if verbose {
            println!(
                "DHCP: Ack IP: {:?}, Server IP: {:?}",
                ack.yiaddr, ack.siaddr
            );
        }
    }

    Ok(())
}

fn main() {
    let mut verbose = false;
    let iface = "eth0";

    //TODO: parse iface from the args
    for arg in env::args().skip(1) {
        match arg.as_ref() {
            "-v" => verbose = true,
            _ => (),
        }
    }

    if let Err(err) = dhcp(iface, verbose) {
        eprintln!("dhcpd: {err}");
        process::exit(1);
    }
}

#[cfg(test)]
mod test {
    use super::MacAddr;

    #[test]
    fn from_str_test() {
        let mac = MacAddr {
            bytes: [0x01, 0x23, 0x45, 0x67, 0x89, 0xab],
        };
        let empty_mac = MacAddr::default();

        assert_eq!(mac, MacAddr::from_str("01:23:45:67:89:ab"));
        assert_eq!(mac, MacAddr::from_str("1:23:45:67:89:ab"));
        assert_eq!(mac, MacAddr::from_str("01:23:45:67:89:AB"));
        assert_eq!(mac, MacAddr::from_str("01-23-45-67-89-ab"));
        assert_eq!(empty_mac, MacAddr::from_str(""));
        assert_eq!(empty_mac, MacAddr::from_str("01:23:45:67:89"));
        assert_eq!(empty_mac, MacAddr::from_str("01:23:45:67:89:ab:cd"));
        assert_eq!(empty_mac, MacAddr::from_str("x1:23:45:67:89:ab"));
        assert_eq!(empty_mac, MacAddr::from_str("01:23-45-67-89-ab"));
        assert_eq!(empty_mac, MacAddr::from_str("01-23-45-67-89-ag"));
        assert_eq!(empty_mac, MacAddr::from_str("01.23.45.67.89.ab"));
        assert_eq!(empty_mac, MacAddr::from_str("01234-23-45-67-89-ab"));
        assert_eq!(empty_mac, MacAddr::from_str("01--23-45-67-89-ab"));
        assert_eq!(empty_mac, MacAddr::from_str("12"));
        assert_eq!(empty_mac, MacAddr::from_str("0:0:0:0:0:0"));

        assert_eq!(mac, MacAddr::from_str(&mac.to_string()));
        assert_eq!(empty_mac, MacAddr::from_str(&empty_mac.to_string()));
    }
}
