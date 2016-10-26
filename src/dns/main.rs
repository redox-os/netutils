#![feature(lookup_host)]

use std::{env, process};
use std::io::{stderr, Write};
use std::net::lookup_host;

fn main(){
    if let Some(name) = env::args().nth(1) {
        for addr in lookup_host(&name).unwrap() {
            println!("{}", addr.ip());
        }
    } else {
        write!(stderr(), "dns: no hostname provided\n").unwrap();
        process::exit(1);
    }
}
