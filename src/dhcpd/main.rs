extern crate netutils;
extern crate syscall;

use netutils::{getcfg, setcfg, MacAddr};
use std::{env, time};
use std::net::UdpSocket;

use dhcp::Dhcp;

mod dhcp;

fn dhcp(quiet: bool) {
    let current_mac = MacAddr::from_str(&getcfg("mac").expect("dhcpd: failed to get current mac"));

    let current_ip = getcfg("ip").unwrap_or("0.0.0.0".to_string());
    if ! quiet {
        println!("DHCP: MAC: {} Current IP: {}", current_mac.to_string(), current_ip);
    }

    let tid = time::SystemTime::now().duration_since(time::UNIX_EPOCH).expect("dhcpd: failed to get time").subsec_nanos();

    let socket = UdpSocket::bind((current_ip.as_str(), 68)).expect("dhcpd: failed to bind udp");
    socket.connect("255.255.255.255:67").expect("dhcpd: failed to connect udp");

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
            chaddr: [current_mac.bytes[0], current_mac.bytes[1], current_mac.bytes[2], current_mac.bytes[3], current_mac.bytes[4], current_mac.bytes[5],
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

        let _sent = socket.send(discover_data).expect("dhcpd: failed to send discover");

        if ! quiet {
            println!("DHCP: Sent Discover");
        }
    }

    let mut offer_data = [0; 65536];
    socket.recv(&mut offer_data).expect("dhcpd: failed to receive offer");
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
            setcfg("ip", &format!("{}.{}.{}.{}", offer.yiaddr[0], offer.yiaddr[1], offer.yiaddr[2], offer.yiaddr[3])).expect("dhcpd: failed to set ip");

            if ! quiet {
                let new_ip = getcfg("ip").expect("dhcpd: failed to get ip");
                println!("DHCP: New IP: {}", new_ip);
            }
        }

        if let Some(subnet) = subnet_option {
            setcfg("ip_subnet", &format!("{}.{}.{}.{}", subnet[0], subnet[1], subnet[2], subnet[3])).expect("dhcpd: failed to set ip subnet");

            if ! quiet {
                let new_subnet = getcfg("ip_subnet").expect("dhcpd: failed to get ip subnet");
                println!("DHCP: New Subnet: {}", new_subnet);
            }
        }

        if let Some(router) = router_option {
            setcfg("ip_router", &format!("{}.{}.{}.{}", router[0], router[1], router[2], router[3])).expect("dhcpd: failed to set ip router");

            if ! quiet {
                let new_router = getcfg("ip_router").expect("dhcpd: failed to get ip router");
                println!("DHCP: New Router: {}", new_router);
            }
        }

        if let Some(mut dns) = dns_option {
            if dns[0] == 127 {
                let opendns = [208, 67, 222, 222].to_vec();
                if ! quiet {
                    println!("DHCP: Received sarcastic DNS suggestion {}.{}.{}.{}, using {}.{}.{}.{} instead",
                            dns[0], dns[1], dns[2], dns[3], opendns[0], opendns[1], opendns[2], opendns[3]);
                }
                dns = opendns;
            }

            setcfg("dns", &format!("{}.{}.{}.{}", dns[0], dns[1], dns[2], dns[3])).expect("dhcpd: failed to set dns");

            if ! quiet {
                let new_dns = getcfg("dns").expect("dhcpd: failed to get dns");
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
            chaddr: [current_mac.bytes[0], current_mac.bytes[1], current_mac.bytes[2], current_mac.bytes[3], current_mac.bytes[4], current_mac.bytes[5],
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

        let _sent = socket.send(request_data).expect("dhcpd: failed to send request");

        if ! quiet {
            println!("DHCP: Sent Request");
        }
    }

    {
        let mut ack_data = [0; 65536];
        socket.recv(&mut ack_data).expect("dhcpd: failed to receive ack");
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
        if unsafe { syscall::clone(0).unwrap() } == 0 {
            dhcp(quiet);
        }
    } else {
        dhcp(quiet);
    }
}
