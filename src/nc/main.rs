use std::env;
use std::io::{self, stdin, Read, Write, Result};
use std::net::TcpStream;
use std::str;
use std::thread;

static MAN_PAGE: &'static str = /* @MANSTART{tail} */ r#"
NAME
    nc - Concatenate and redirect sockets
SYNOPSIS
    nc [[-h | --help] | [[-u | --udp]] [hostname:port]
DESCRIPTION
    Netcat (nc) is command line utility which can read and write data across network. Currently
    it only works with IPv4 and does not support any encryption.
OPTIONS
    -h
    --help
        Print this manual page.
    -u
    --udp
        Use UDP instead of default TCP. Not implemented yet.

    -l
    --listen
        Listen for incoming connections. Not implemented yet.
AUTHOR
    Written by Sehny.
"#; /* @MANEND */

const BUFFER_SIZE: usize = 65636;

enum TransportProtocol {
    Tcp,
    Udp,
}

// enum NcMode {
//     Connect,
//     Listen,
// }

fn connect_tcp(host: String) -> Result<()> {
    let mut stream_read = TcpStream::connect(host.as_str()).unwrap();
    let mut stream_write = stream_read.try_clone().unwrap();

    thread::spawn(move || {
        loop {
            let mut buffer = [0u8; BUFFER_SIZE];
            let count  = stream_read.read(&mut buffer).unwrap();
            print!("{}", unsafe { str::from_utf8_unchecked(&buffer[..count]) });
        }
    });

    loop {
        let mut buffer = [0; BUFFER_SIZE];
        let count = stdin().read(&mut buffer).unwrap();
        let _ = stream_write.write(&buffer[..count]).unwrap();
    }
}

fn main() {

    let mut args = env::args().skip(1);
    let mut hostname = "".to_string();
    let mut proto = TransportProtocol::Tcp;
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
                    println!("This functionality has not been implemented yet.");
                    return;
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

    println!("Remote host: {}", hostname);
    match proto {
        TransportProtocol::Tcp => connect_tcp(hostname).unwrap(),
        TransportProtocol::Udp => println!("Not implemented. udp"),
    }
}
