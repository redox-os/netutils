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
        match client.get(&url).send() {
            Ok(mut res) => {
                let mut data = Vec::new();
                match res.read_to_end(&mut data) {
                    Ok(_) => match stdout().write(&data) {
                        Ok(_) => (),
                        Err(err) => {
                            writeln!(stderr(), "wget: failed to write output: {}", err).unwrap();
                            process::exit(1);
                        }
                    },
                    Err(err) => {
                        writeln!(stderr(), "wget: failed to read response: {}", err).unwrap();
                        process::exit(1);
                    }
                }
            },
            Err(err) => {
                writeln!(stderr(), "wget: failed to send request: {}", err).unwrap();
                process::exit(1);
            }
        }
    } else {
        writeln!(stderr(), "wget http://host:port/path").unwrap();
        process::exit(1);
    }
}
