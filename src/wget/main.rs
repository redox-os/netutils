extern crate hyper;
extern crate hyper_rustls;

use std::env;
use std::io::{stderr, stdout, Read, Write};
use std::process;
use hyper::Client;
use hyper::net::HttpsConnector;

fn main() {
    if let Some(url) = env::args().nth(1) {
        let client = Client::with_connector(HttpsConnector::new(hyper_rustls::TlsClient::new()));
        let mut res = client.get(&url).send().unwrap();
        let mut data = Vec::new();
        res.read_to_end(&mut data).unwrap();
        stdout().write(&data).unwrap();
    } else {
        write!(stderr(), "wget: http://host:port/path\n").unwrap();
        process::exit(1);
    }
}
