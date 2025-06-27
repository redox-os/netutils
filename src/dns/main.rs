use std::net::ToSocketAddrs;
use std::{env, process};

fn main() {
    if let Some(name) = env::args().nth(1) {
        for addr in (name.as_str(), 0).to_socket_addrs().unwrap() {
            println!("{}", addr.ip());
        }
    } else {
        eprintln!("dns: no hostname provided\n");
        process::exit(1);
    }
}
