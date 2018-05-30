#![deny(warnings)]

extern crate arg_parser;
extern crate reqwest;
extern crate pbr;

use std::env;
use std::fs::File;
use std::io::{self, Read, Write};
use std::process;
use arg_parser::ArgParser;
use pbr::{ProgressBar, Units};
use reqwest::{StatusCode, header::ContentLength};

fn wget<W: Write>(url: &str, mut output: W) {
    let mut stderr = io::stderr();

    match reqwest::get(url) {
        Ok(mut response) => match response.status() {
            StatusCode::Ok => {
                let mut count = 0;
                let length = response.headers().get::<ContentLength>().map_or(0, |h| h.0 as usize);

                let mut pb = ProgressBar::on(io::stderr(), length as u64);
                pb.set_units(Units::Bytes);
                loop {
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
                    match output.write_all(&buf[.. res]) {
                        Ok(res) => res,
                        Err(err) => {
                            writeln!(stderr, "wget: failed to write data: {}", err).unwrap();
                            process::exit(1);
                        }
                    };
                    count += res;
                    pb.set(count as u64);
                }
            },
            _ => {
                let _ = writeln!(stderr, "wget: failed to receive request: {}", response.status());
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
                        let _ = writeln!(io::stderr(), "wget: failed to sync data: {}", err);
                        process::exit(1);
                    }
                },
                Err(err) => {
                    writeln!(io::stderr(), "wget: failed to create '{}': {}", path, err).unwrap();
                    process::exit(1);
                }
            },
            None => {
                wget(&url, io::stdout());
            }
        },
        None => {
            writeln!(io::stderr(), "wget http://host:port/path [-O output]").unwrap();
            process::exit(1);
        }
    }
}
