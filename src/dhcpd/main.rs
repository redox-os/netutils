extern crate netutils;

use netutils::MacAddr;
use std::{env, process, time};
use std::io::{self, Read, Write};
use std::fs::{File, OpenOptions};
use std::net::{SocketAddr, UdpSocket};
use std::time::Duration;

use dhcp::Dhcp;

mod dhcp;

macro_rules! try_fmt {
    ($e:expr, $m:expr) =>(
        match $e {
            Ok(ok) => ok,
            Err(err) => return Err(format!("{}: {}", $m, err)),
        }
    )
}

fn get_cfg_value(path: &str) -> Result<String, String> {
    let path = format!("/scheme/netcfg/{}", path);
    let mut file = File::open(&path).map_err(|_| format!("Can't open {}", &path))?;
    let mut result = String::new();
    file.read_to_string(&mut result)
        .map_err(|_| format!("Can't read {}", path))?;
    Ok(result)
}

fn get_iface_cfg_value(iface: &str, cfg: &str) -> Result<String, String> {
    let path = format!("ifaces/{}/{}", iface, cfg);
    get_cfg_value(&path)
}

fn set_cfg_value(path: &str, value: &str) -> Result<(), String> {
    let path = format!("/scheme/netcfg/{}", path);
    let mut file = OpenOptions::new().read(false).write(true).create(false).open(&path)
        .map_err(|_| format!("Can't open {}", path))?;
    file.write(value.as_bytes())
        .map(|_| ())
        .map_err(|_| format!("Can't write {} to {}", value, path))?;
    file.sync_data()
        .map_err(|_| format!("Can't commit {} to {}", value, path))
}

fn set_iface_cfg_value(iface: &str, cfg: &str, value: &str) -> Result<(), String> {
    let path = format!("ifaces/{}/{}", iface, cfg);
    set_cfg_value(&path, value)
}

fn dhcp(iface: &str, quiet: bool) -> Result<(), String> {
    let current_mac = MacAddr::from_str(get_iface_cfg_value(iface, "mac")?.trim());

    let current_ip = get_iface_cfg_value(iface, "addr/list")?
        .lines()
        .next()
        .map(|l| l.to_owned())
        .unwrap_or("0.0.0.0".to_string());

    if !quiet {
        println!(
            "DHCP: MAC: {} Current IP: {}",
            current_mac.to_string(),
            current_ip.trim()
        );
    }

    let tid = try_fmt!(
        time::SystemTime::now().duration_since(time::UNIX_EPOCH),
        "failed to get time"
    ).subsec_nanos();

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
            53,
            1,
            1,

            // End
            255
        ].iter().zip(discover.options.iter_mut()) {
            *d = *s;
        }

        let discover_data = unsafe {
            std::slice::from_raw_parts(
                (&discover as *const Dhcp) as *const u8,
                std::mem::size_of::<Dhcp>(),
            )
        };

        let _sent = try_fmt!(socket.send(discover_data), "failed to send discover");

        if !quiet {
            println!("DHCP: Sent Discover");
        }
    }

    let mut offer_data = [0; 65536];
    try_fmt!(socket.recv(&mut offer_data), "failed to receive offer");
    let offer = unsafe { &*(offer_data.as_ptr() as *const Dhcp) };
    if !quiet {
        println!(
            "DHCP: Offer IP: {:?}, Server IP: {:?}",
            offer.yiaddr, offer.siaddr
        );
    }

    {
        let mut subnet_option = None;
        let mut router_option = None;
        let mut dns_option = None;

        let mut options = offer.options.iter();
        while let Some(option) = options.next() {
            match *option {
                0 => (),
                255 => break,
                _ => if let Some(len) = options.next() {
                    if *len as usize <= options.as_slice().len() {
                        let data = &options.as_slice()[..*len as usize];
                        for _data_i in 0..*len {
                            options.next();
                        }
                        match *option {
                            1 => {
                                if !quiet {
                                    println!("DHCP: Subnet Mask: {:?}", data);
                                }
                                if data.len() == 4 && subnet_option.is_none() {
                                    subnet_option = Some(Vec::from(data));
                                }
                            }
                            3 => {
                                if !quiet {
                                    println!("DHCP: Router: {:?}", data);
                                }
                                if data.len() == 4 && router_option.is_none() {
                                    router_option = Some(Vec::from(data));
                                }
                            }
                            6 => {
                                if !quiet {
                                    println!("DHCP: Domain Name Server: {:?}", data);
                                }
                                if data.len() == 4 && dns_option.is_none() {
                                    dns_option = Some(Vec::from(data));
                                }
                            }
                            51 => {
                                if !quiet {
                                    println!("DHCP: Lease Time: {:?}", data);
                                }
                            }
                            53 => {
                                if !quiet {
                                    println!("DHCP: Message Type: {:?}", data);
                                }
                            }
                            54 => {
                                if !quiet {
                                    println!("DHCP: Server ID: {:?}", data);
                                }
                            }
                            _ => {
                                if !quiet {
                                    println!("DHCP: {}: {:?}", option, data);
                                }
                            }
                        }
                    }
                },
            }
        }

        let mask_len = if let Some(subnet) = subnet_option {
            let mut subnet: u32 = (subnet[0] as u32) << 24 | (subnet[1] as u32) << 16 |
                                  (subnet[2] as u32) << 8 | subnet[3] as u32;
            subnet = !subnet;
            subnet.leading_zeros()
        } else {
            0
        };

        let new_ips = format!("{}.{}.{}.{}/{}\n",
                              offer.yiaddr[0], offer.yiaddr[1], offer.yiaddr[2], offer.yiaddr[3], mask_len);
        try_fmt!(
            set_iface_cfg_value(iface, "addr/set", &new_ips),
            "failed to set ip"
        );

        if !quiet {
            let new_ip = try_fmt!(get_iface_cfg_value(iface, "addr/list"), "failed to get ip");
            println!("DHCP: New IP: {}", new_ip.trim());
        }

        if let Some(router) = router_option {
            let default_route = format!("default via {}.{}.{}.{}",
                                        router[0], router[1], router[2], router[3]);

            try_fmt!(
                set_cfg_value("route/add", &default_route),
                "failed to set default route"
            );

            if !quiet {
                let new_router = try_fmt!(get_cfg_value("route/list"), "failed to get ip router");
                println!("DHCP: New Router: {}", new_router.trim());
            }
        }

        if let Some(mut dns) = dns_option {
            if dns[0] == 127 {
                let opendns = [208, 67, 222, 222].to_vec();
                if !quiet {
                    println!("DHCP: Received sarcastic DNS suggestion {}.{}.{}.{}, using {}.{}.{}.{} instead",
                            dns[0], dns[1], dns[2], dns[3], opendns[0], opendns[1], opendns[2], opendns[3]);
                }
                dns = opendns;
            }

            let nameserver = format!("{}.{}.{}.{}", dns[0], dns[1], dns[2], dns[3]);

            try_fmt!(
                set_cfg_value("resolv/nameserver", &nameserver),
                "failed to set name server"
            );

            if !quiet {
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

            // Server IP Address
            54,
            4,
            offer.siaddr[0],
            offer.siaddr[1],
            offer.siaddr[2],
            offer.siaddr[3],

            // End
            255,
        ].iter()
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

        if !quiet {
            println!("DHCP: Sent Request");
        }
    }

    {
        let mut ack_data = [0; 65536];
        try_fmt!(socket.recv(&mut ack_data), "failed to receive ack");
        let ack = unsafe { &*(ack_data.as_ptr() as *const Dhcp) };
        if !quiet {
            println!(
                "DHCP: Ack IP: {:?}, Server IP: {:?}",
                ack.yiaddr, ack.siaddr
            );
        }
    }

    Ok(())
}

fn main() {
    let mut background = false;
    let mut quiet = false;
    let iface = "eth0";

    //TODO: parse iface from the args
    for arg in env::args().skip(1) {
        match arg.as_ref() {
            "-b" => background = true,
            "-q" => quiet = true,
            _ => (),
        }
    }

    if background {
        redox_daemon::Daemon::new(move |daemon| {
            daemon.ready().expect("failed to signal readiness");

            if let Err(err) = dhcp(iface, quiet) {
                writeln!(io::stderr(), "dhcpd: {}", err).unwrap();
                process::exit(1);
            }
            process::exit(0);
        }).expect("dhcpd: failed to daemonize");
    } else {
        if let Err(err) = dhcp(iface, quiet) {
            println!("Error {}", err);
            writeln!(io::stderr(), "dhcpd: {}", err).unwrap();
            process::exit(1);
        }
    }
}
