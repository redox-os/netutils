#![deny(warnings)]

extern crate arg_parser;
extern crate hyper;
extern crate hyper_rustls;

use std::env;
use std::fs::File;
use std::io::{stderr, stdout, Read, Write};
use std::process;
use std::time::Duration;
use hyper::Client;
use hyper::net::HttpsConnector;
use hyper::header::ContentLength;
use hyper::status::StatusCode;
use arg_parser::ArgParser;

fn wget<W: Write>(url: &str, mut output: W) {
    let mut stderr = stderr();

    let mut client = Client::with_connector(HttpsConnector::new(hyper_rustls::TlsClient::new()));
    client.set_read_timeout(Some(Duration::new(5, 0)));
    client.set_write_timeout(Some(Duration::new(5, 0)));
    match client.get(url).send() {
        Ok(mut response) => match response.status {
            StatusCode::Ok => {
                let mut count = 0;
                let length = response.headers.get::<ContentLength>().map_or(0, |h| h.0 as usize);

                let mut status = [b' '; 50];

                loop {
                    let (percent, cols) = if count >= length {
                        (100, status.len())
                    } else {
                        ((100 * count) / length, (status.len() * count) / length)
                    };

                    let _ = write!(stderr, "\r* {:>3}% [", percent);

                    for i in 0..cols {
                        status[i] = b'=';
                    }
                    if cols < status.len() {
                        status[cols] = b'>';
                    }

                    let _ = stderr.write(&status);

                    let (size, suffix) = if count >= 10 * 1000 * 1000 * 1000 {
                        (count / (1000 * 1000 * 1000), "GB")
                    } else if count >= 10 * 1000 * 1000 {
                        (count / (1000 * 1000), "MB")
                    } else if count >= 10 * 1000 {
                        (count / 1000, "KB")
                    } else {
                        (count, "B")
                    };

                    let _ = write!(stderr, "] {:>4} {}", size, suffix);

                    let mut buf = [0; 8192];
                    let res = match response.read(&mut buf) {
                        Ok(res) => res,
                        Err(err) => {
                            writeln!(stderr, "wget: failed to read data: {}", err).unwrap();
                            process::exit(1);
                        }
                    };
                    if res == 0 {
                        break;
                    }
                    count += match output.write(&buf[.. res]) {
                        Ok(res) => res,
                        Err(err) => {
                            writeln!(stderr, "wget: failed to write data: {}", err).unwrap();
                            process::exit(1);
                        }
                    };
                }
                let _ = write!(stderr, "\n");
            },
            _ => {
                let _ = writeln!(stderr, "wget: failed to receive request: {}", response.status);
                process::exit(1);
            }
        },
        Err(err) => {
            let _ = writeln!(stderr, "wget: failed to send request: {}", err);
            process::exit(1);
        }
    }
}

fn main() {
    let mut parser = ArgParser::new(1)
        .add_opt("O", "output-document");
    parser.parse(env::args());

    match parser.args.get(0) {
        Some(url) => match parser.get_opt("output-document") {
            Some(path) => match File::create(&path) {
                Ok(mut file) => {
                    wget(&url, &mut file);
                    if let Err(err) = file.sync_all() {
                        let _ = writeln!(stderr(), "wget: failed to sync data: {}", err);
                        process::exit(1);
                    }
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
            writeln!(stderr(), "wget http://host:port/path [-O output]").unwrap();
            process::exit(1);
        }
    }
}
