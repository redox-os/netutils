use std::{env, process};
use std::io::{stderr, Write};
use std::net::ToSocketAddrs;

fn main(){
    if let Some(name) = env::args().nth(1) {
        for addr in (name.as_str(), 0).to_socket_addrs().unwrap() {
            println!("{}", addr.ip());
        }
    } else {
        write!(stderr(), "dns: no hostname provided\n").unwrap();
        process::exit(1);
    }
}
