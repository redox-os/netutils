use std::{env, thread, time};
use std::fs::File;
use std::io::{Read, Write};

use dhcp::Dhcp;

mod dhcp;

fn getcfg(key: &str) -> String {
    let mut value = String::new();
    File::open(&format!("/etc/net/{}", key)).unwrap().read_to_string(&mut value).unwrap();
    value.trim().to_string()
}

fn setcfg(key: &str, value: &str) {
    File::create(&format!("/etc/net/{}", key)).unwrap().write_all(value.as_bytes()).unwrap();
}

fn dhcp(quiet: bool) {
    let current_mac: Vec<u8> = getcfg("mac").split(".").map(|part| part.parse::<u8>().unwrap_or(0)).collect();

    {
        if ! quiet {
            let current_ip = getcfg("ip");
            println!("DHCP: Current IP: {}", current_ip);
        }
    }

    let tid = time::SystemTime::now().duration_since(time::UNIX_EPOCH).unwrap().subsec_nanos();

    let mut socket = File::open("udp:255.255.255.255:67/68").unwrap();

    {
        let mut discover = Dhcp {
            op: 1,
            htype: 1,
            hlen: 6,
            hops: 0,
            tid: tid,
            secs: 0,
            flags: 0x8000u16.to_be(),
            ciaddr: [0, 0, 0, 0],
            yiaddr: [0, 0, 0, 0],
            siaddr: [0, 0, 0, 0],
            giaddr: [0, 0, 0, 0],
            chaddr: [current_mac[0], current_mac[1], current_mac[2], current_mac[3], current_mac[4], current_mac[5],
                    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00],
            sname: [0; 64],
            file: [0; 128],
            magic: 0x63825363u32.to_be(),
            options: [0; 308]
        };

        for (s, mut d) in [53, 1, 1, 255].iter().zip(discover.options.iter_mut()) {
            *d = *s;
        }

        let discover_data = unsafe { std::slice::from_raw_parts((&discover as *const Dhcp) as *const u8, std::mem::size_of::<Dhcp>()) };

        let _sent = socket.write(discover_data).unwrap();

        if ! quiet {
            println!("DHCP: Sent Discover");
        }
    }

    let mut offer_data = [0; 65536];
    socket.read(&mut offer_data).unwrap();
    let offer = unsafe { &* (offer_data.as_ptr() as *const Dhcp) };
    if ! quiet {
        println!("DHCP: Offer IP: {:?}, Server IP: {:?}", offer.yiaddr, offer.siaddr);
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
                        let data = &options.as_slice()[.. *len as usize];
                        for _data_i in 0..*len {
                            options.next();
                        }
                        match *option {
                            1 => {
                                if ! quiet {
                                    println!("DHCP: Subnet Mask: {:?}", data);
                                }
                                if data.len() == 4 && subnet_option.is_none() {
                                    subnet_option = Some(Vec::from(data));
                                }
                            },
                            3 => {
                                if ! quiet {
                                    println!("DHCP: Router: {:?}", data);
                                }
                                if data.len() == 4  && router_option.is_none() {
                                    router_option = Some(Vec::from(data));
                                }
                            },
                            6 => {
                                if ! quiet {
                                    println!("DHCP: Domain Name Server: {:?}", data);
                                }
                                if data.len() == 4 && dns_option.is_none() {
                                    dns_option = Some(Vec::from(data));
                                }
                            },
                            51 => {
                                if ! quiet {
                                    println!("DHCP: Lease Time: {:?}", data);
                                }
                            },
                            53 => {
                                if ! quiet {
                                    println!("DHCP: Message Type: {:?}", data);
                                }
                            },
                            54 => {
                                if ! quiet {
                                    println!("DHCP: Server ID: {:?}", data);
                                }
                            },
                            _ => {
                                if ! quiet {
                                    println!("DHCP: {}: {:?}", option, data);
                                }
                            }
                        }
                    }
                },
            }
        }

        {
            setcfg("ip", &format!("{}.{}.{}.{}", offer.yiaddr[0], offer.yiaddr[1], offer.yiaddr[2], offer.yiaddr[3]));

            if ! quiet {
                let new_ip = getcfg("ip");
                println!("DHCP: New IP: {}", new_ip);
            }
        }

        if let Some(subnet) = subnet_option {
            setcfg("ip_subnet", &format!("{}.{}.{}.{}", subnet[0], subnet[1], subnet[2], subnet[3]));

            if ! quiet {
                let new_subnet = getcfg("ip_subnet");
                println!("DHCP: New Subnet: {}", new_subnet);
            }
        }

        if let Some(router) = router_option {
            setcfg("ip_router", &format!("{}.{}.{}.{}", router[0], router[1], router[2], router[3]));

            if ! quiet {
                let new_router = getcfg("ip_router");
                println!("DHCP: New Router: {}", new_router);
            }
        }

        if let Some(dns) = dns_option {
            setcfg("dns", &format!("{}.{}.{}.{}", dns[0], dns[1], dns[2], dns[3]));

            if ! quiet {
                let new_dns = getcfg("dns");
                println!("DHCP: New DNS: {}", new_dns);
            }
        }
    }

    {
        let mut request = Dhcp {
            op: 1,
            htype: 1,
            hlen: 6,
            hops: 0,
            tid: tid,
            secs: 0,
            flags: 0,
            ciaddr: [0; 4],
            yiaddr: [0; 4],
            siaddr: offer.siaddr,
            giaddr: [0; 4],
            chaddr: [current_mac[0], current_mac[1], current_mac[2], current_mac[3], current_mac[4], current_mac[5],
                    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00],
            sname: [0; 64],
            file: [0; 128],
            magic: 0x63825363u32.to_be(),
            options: [0; 308]
        };

        for (s, mut d) in [53, 1, 3, 50, 4, offer.yiaddr[0], offer.yiaddr[1], offer.yiaddr[2], offer.yiaddr[3], 255].iter().zip(request.options.iter_mut()) {
            *d = *s;
        }

        let request_data = unsafe { std::slice::from_raw_parts((&request as *const Dhcp) as *const u8, std::mem::size_of::<Dhcp>()) };

        let _sent = socket.write(request_data).unwrap();

        if ! quiet {
            println!("DHCP: Sent Request");
        }
    }

    {
        let mut ack_data = [0; 65536];
        socket.read(&mut ack_data).unwrap();
        let ack = unsafe { &* (ack_data.as_ptr() as *const Dhcp) };
        if ! quiet {
            println!("DHCP: Ack IP: {:?}, Server IP: {:?}", ack.yiaddr, ack.siaddr);
        }
    }
}

fn main(){
    let mut background = false;
    let mut quiet = false;
    for arg in env::args().skip(1) {
        match arg.as_ref() {
            "-b" => background = true,
            "-q" => quiet = true,
            _ => ()
        }
    }

    if background {
        thread::spawn(move || {
            dhcp(quiet);
        });
    } else {
        dhcp(quiet);
    }
}
