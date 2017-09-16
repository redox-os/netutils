//#![deny(warnings)]

extern crate arg_parser;
extern crate futures;
extern crate hyper;
extern crate hyper_rustls;
extern crate pbr;
extern crate tokio_core;

use std::env;
use std::fs::File;
use std::io::{self, Write};
use std::process;
use std::str::FromStr;

use futures::future::Future;
use futures::Stream;
use hyper::Client;
use hyper::header::ContentLength;
use hyper::{StatusCode, Uri};
use arg_parser::ArgParser;
use pbr::{ProgressBar, Units};

fn wget<W: Write>(url: &str, mut output: W) {
    let uri = match Uri::from_str(url) {
        Ok(uri) => uri,
        Err(err) => {
            writeln!(io::stderr(), "wget: invalid URL: {}", err).unwrap();
            process::exit(1);
        }
    };

    let mut core = tokio_core::reactor::Core::new().unwrap();
    let handle = core.handle();
    let https = hyper_rustls::HttpsConnector::new(1, &handle);
    let client = Client::configure().connector(https).build(&handle);

    let work = client.get(uri).and_then(|response| {
        match response.status() {
            StatusCode::Ok => {
                let mut count = 0;
                let length = response.headers().get::<ContentLength>().map_or(0, |h| h.0 as usize);

                let mut pb = ProgressBar::on(io::stderr(), length as u64);
                pb.set_units(Units::Bytes);

                response.body().for_each(move |chunk| {
                    let res = output.write_all(&chunk).map(|_| ()).map_err(From::from);
                    if res.is_ok() {
                        count += chunk.len();
                        pb.set(count as u64);
                    }
                    res
                })
            },
            _ => {
                let _ = writeln!(io::stderr(), "wget: failed to receive request: {}", response.status());
                process::exit(1);
            }
        }
    });

    if let Err(err) = core.run(work) {
        let _ = writeln!(io::stderr(), "wget: failed to download: {}", err);
        std::process::exit(1)
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
