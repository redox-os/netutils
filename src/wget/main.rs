extern crate hyper;
extern crate hyper_rustls;

use std::env;
use std::fs::File;
use std::io::{self, stderr, stdout, Write};
use std::process;
use std::time::Duration;
use hyper::Client;
use hyper::net::HttpsConnector;
use hyper::status::StatusCode;

fn wget<W: Write>(url: &str, mut output: W) {
    let mut client = Client::with_connector(HttpsConnector::new(hyper_rustls::TlsClient::new()));
    client.set_read_timeout(Some(Duration::new(5, 0)));
    client.set_write_timeout(Some(Duration::new(5, 0)));
    match client.get(url).send() {
        Ok(mut res) => match res.status {
            StatusCode::Ok => match io::copy(&mut res, &mut output) {
                Ok(_) => {
                    let _ = output.flush();
                },
                Err(err) => {
                    writeln!(stderr(), "wget: failed to transfer data: {}", err).unwrap();
                    process::exit(1);
                }
            },
            _ => {
                writeln!(stderr(), "wget: failed to receive request: {}", res.status).unwrap();
                process::exit(1);
            }
        },
        Err(err) => {
            writeln!(stderr(), "wget: failed to send request: {}", err).unwrap();
            process::exit(1);
        }
    }
}

fn main() {
    let mut args = env::args().skip(1);
    match args.next() {
        Some(url) => match args.next() {
            Some(path) => match File::create(&path) {
                Ok(file) => {
                    wget(&url, file);
                },
                Err(err) => {
                    writeln!(stderr(), "wget: failed to create '{}': {}", path, err).unwrap();
                    process::exit(1);
                }
            },
            None => {
                wget(&url, stdout());
            }
        },
        None => {
            writeln!(stderr(), "wget http://host:port/path [output]").unwrap();
            process::exit(1);
        }
    }
}
