use std::io::{Read, Write};
use std::net::TcpListener;
use std::{str, thread};

fn main() {
    thread::spawn(|| {
        let listener = TcpListener::bind("0.0.0.0:8080").unwrap();
        loop {
            let mut stream = listener.accept().unwrap().0;

            let mut info = String::new();

            let addresses = format!("{:?}: {:?}\n", stream.local_addr().unwrap(), stream.peer_addr().unwrap());

            info.push_str(&addresses);

            let mut data = [0; 65536];
            let count = stream.read(&mut data).unwrap();
            info.push_str(&format!("Read {}\n", count));

            let request = str::from_utf8(&data[.. count]).unwrap();
            info.push_str(&format!("{}\n", request));

            let count = stream.write(addresses.as_bytes()).unwrap();
            info.push_str(&format!("Wrote {}\n", count));

            print!("{}", info);
        }
    });
}
