use std::env;
use std::io::{self, Write};

mod modes;
use modes::*;

static MAN_PAGE: &'static str = /* @MANSTART{nc} */ r#"
NAME
    nc - Concatenate and redirect sockets
SYNOPSIS
    nc [[-h | --help] | [-u | --udp] | [-l | --listen]] [hostname:port]
DESCRIPTION
    Netcat (nc) is command line utility which can read and write data across network. Currently
    it only works with IPv4 and does not support any encryption.
OPTIONS
    -h
    --help
        Print this manual page.
    -u
    --udp
        Use UDP instead of default TCP.

    -l
    --listen
        Listen for incoming connections.
AUTHOR
    Written by Sehny.
"#; /* @MANEND */

enum TransportProtocol {
    Tcp,
    Udp,
}

enum NcMode {
    Connect,
    Listen,
}

fn main() {

    let mut args = env::args().skip(1);
    let mut hostname = "".to_string();
    let mut proto = TransportProtocol::Tcp;
    let mut mode = NcMode::Connect;
    let mut stdout = io::stdout();

    while let Some(arg) = args.next() {
        if arg.starts_with('-') {
            match arg.as_str() {
                "-h" | "--help" => {
                    stdout.write_all(MAN_PAGE.as_bytes()).unwrap();
                    return;
                }
                "-u" | "--udp" => proto = TransportProtocol::Udp,
                "-l" | "--listen" => {
                    mode = NcMode::Listen;
                }
                _ => {
                    println!("Invalid argument!");
                    return;
                }
            }
        } else {
            hostname = arg;
        }
    }

    match (mode, proto) {
        (NcMode::Connect, TransportProtocol::Tcp) => {
            connect_tcp(&hostname).unwrap_or_else(|e| {
                println!("nc error: {}", e);
            });
        }
        (NcMode::Listen, TransportProtocol::Tcp) => {
            listen_tcp(&hostname).unwrap_or_else(|e| {
                println!("nc error: {}", e);
            });
        }
        (NcMode::Connect, TransportProtocol::Udp) => {
            connect_udp(&hostname).unwrap_or_else(|e| {
                println!("nc error: {}", e);
            });
        }
        (NcMode::Listen, TransportProtocol::Udp) => {
            listen_udp(&hostname).unwrap_or_else(|e| {
                println!("nc error: {}", e);
            });
        }
    }

}
