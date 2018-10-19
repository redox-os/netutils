#![deny(warnings)]

extern crate arg_parser;
extern crate hyper;
extern crate hyper_rustls;
extern crate pbr;
extern crate url;

use std::env;
use std::fs::File;
use std::io::{self, Read, Write};
use std::process;
use std::time::Duration;
use hyper::Client;
use hyper::net::HttpsConnector;
use hyper::header::ContentLength;
use hyper::status::StatusCode;
use arg_parser::ArgParser;
use pbr::{ProgressBar, Units};
use url::Url;

enum WgetOutput {
    File { path: String },
    Stdout,
}

fn wget<W: Write>(url: &str, mut output: W) {
    let mut stderr = io::stderr();

    let mut client = Client::with_connector(HttpsConnector::new(hyper_rustls::TlsClient::new()));
    client.set_read_timeout(Some(Duration::new(5, 0)));
    client.set_write_timeout(Some(Duration::new(5, 0)));
    match client.get(url).send() {
        Ok(mut response) => match response.status {
            StatusCode::Ok => {
                let mut count = 0;
                let length = response.headers.get::<ContentLength>().map_or(0, |h| h.0 as usize);

                let mut pb = ProgressBar::on(io::stderr(), length as u64);
                pb.set_units(Units::Bytes);
                loop {
                    let mut buf = [0; 8192];
                    let res = match response.read(&mut buf) {
                        Ok(res) => res,
                        Err(err) => {
                            let _ = writeln!(stderr, "wget: failed to read data: {}", err);
                            process::exit(1);
                        }
                    };
                    if res == 0 {
                        break;
                    }
                    count += match output.write(&buf[.. res]) {
                        Ok(res) => res,
                        Err(err) => {
                            let _ = writeln!(stderr, "wget: failed to write data: {}", err);
                            process::exit(1);
                        }
                    };
                    pb.set(count as u64);
                }
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
        Some(url) => {
            let output = match parser.get_opt("output-document") {
                Some(path) => {
                    if path == "-" {
                        WgetOutput::Stdout
                    } else {
                        WgetOutput::File { path }
                    }
                },
                None => {
                    match Url::parse(url) {
                        Ok(parsed_url) => {
                            let path = match parsed_url.path_segments() {
                                Some(path_segments) => path_segments.last().unwrap_or(""),
                                None => "",
                            }.to_string();

                            if path.is_empty() {
                                let _ = writeln!(io::stderr(), "wget: failed to derive output path from url");
                                process::exit(1);
                            } else {
                                WgetOutput::File { path }
                            }
                        },
                        Err(err) => {
                            let _ = writeln!(io::stderr(), "wget: failed to parse url: {}", err);
                            process::exit(1);
                        }
                    }
                }
            };

            match output {
                WgetOutput::File { path } => match File::create(&path) {
                    Ok(mut file) => {
                        wget(&url, &mut file);
                        if let Err(err) = file.sync_all() {
                            let _ = writeln!(io::stderr(), "wget: failed to sync data: {}", err);
                            process::exit(1);
                        }
                    },
                    Err(err) => {
                        let _ = writeln!(io::stderr(), "wget: failed to create '{}': {}", path, err);
                        process::exit(1);
                    }
                },
                WgetOutput::Stdout => {
                    wget(&url, io::stdout());
                }
            }
        },
        None => {
            let _ = writeln!(io::stderr(), "wget http://host:port/path [-O output]");
            process::exit(1);
        }
    }
}
