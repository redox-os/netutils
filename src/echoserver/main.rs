use std::env;
use std::net::UdpSocket;

pub fn run() -> Result<(), String> {
    let mut args = env::args().skip(1);
    let local_endpoint = args.next().ok_or("Missing argument".to_owned())?;
    let mut socket = UdpSocket::bind(local_endpoint).map_err(|e| format!("Failed to start UDP server: {}", e))?;
    let mut buf = [0; 1000];
    loop {
        let (count, src) = socket.recv_from(&mut buf).map_err(|e| format!("Failed to receive data: {}", e))?;
        println!("Received {} bytes from {:?}", count, src);
        socket.send_to(&buf[..count], &src).map_err(|e| format!("Failed to send data: {}", e))?;
    }
    return Ok(());
}

fn main() {
    match run() {
        Ok(_) => {
            println!("Exiting...");
        },
        Err(e) => {
            println!("Error while running udp server: {}", e);
        }
    }
}
