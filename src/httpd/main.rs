#![cfg_attr(not(target_os = "redox"), feature(libc))]

extern crate hyper;
extern crate futures;

use futures::future::FutureResult;

use hyper::{Method, StatusCode};
use hyper::header::{ContentLength, ContentType, Headers};
use hyper::server::{Http, Service, Request, Response};

use std::env;
use std::fs::{self, File};
use std::io::{Error, ErrorKind, Result, Read};
use std::path::{Path, PathBuf};

fn read_dir(root: &Path, path: &Path) -> Result<(Headers, Vec<u8>)> {
    let mut names = vec![];
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        if let Some(name) = entry.file_name().to_str() {
            names.push(name.to_string());
        }
    }

    let mut response = String::new();
    response.push_str("<!DOCTYPE html>\n<html><body>");
    if let Ok(relative) = path.strip_prefix(root){
        if let Some(href) = relative.to_str() {
            if ! href.is_empty() {
                names.push("..".to_string());
            }
            response.push_str("<h1>Index of /");
            response.push_str(href);
            response.push_str("</h1>\n");
        }
    }

    names.sort();
    for name in names {
        let mut name_path = path.to_path_buf();
        name_path.push(&name);
        if let Ok(relative) = name_path.as_path().strip_prefix(root) {
            if let Some(href) = relative.to_str() {
                response.push_str("<a href='/");
                response.push_str(href);
                response.push_str("'>");
                response.push_str(&name);
                response.push_str("</a><br/>\n");
            } else {
                response.push_str(&name);
                response.push_str("<br/>\n");
            }
        } else {
            response.push_str(&name);
            response.push_str("<br/>\n");
        }
    }
    response.push_str("</body></html>");

    let mut headers = Headers::new();
    headers.set(ContentType("text/html".parse().unwrap()));
    headers.set(ContentLength(response.len() as u64));

    Ok((headers, response.into_bytes()))
}

fn read_file(_root: &Path, path: &Path) -> Result<(Headers, Vec<u8>)> {
    let mut file = File::open(path)?;

    let mut response = Vec::new();
    file.read_to_end(&mut response)?;

    let extension = path.extension().map_or("", |ext_os| ext_os.to_str().unwrap_or(""));
    let mime_type = match extension {
        "css" => "text/css",
        "html" => "text/html",
        "js" => "text/javascript",
        "jpg" | "jpeg" => "text/jpeg",
        "png" => "image/png",
        "svg" => "image/svg+xml",
        _ => "text/plain"
    };

    let mut headers = Headers::new();
    headers.set(ContentType(mime_type.parse().unwrap()));
    headers.set(ContentLength(response.len() as u64));

    Ok((headers, response))
}

fn read_path(root: &Path, path: &Path) -> Result<(Headers, Vec<u8>)> {
    if path.is_dir() {
        let mut index_path = path.to_path_buf();
        index_path.push("index.html");
        if index_path.is_file() {
            read_file(root, &index_path)
        } else {
            read_dir(root, path)
        }
    } else {
        read_file(root, path)
    }
}

fn read_req(root: &Path, request: &Request) -> Result<(Headers, Vec<u8>)> {
    let uri = request.uri();
    let path = uri.path();
    let mut full_path = root.to_path_buf();
    full_path.push(path.trim_left_matches('/'));
    if full_path.as_path().strip_prefix(root).is_ok() {
        read_path(root, &full_path)
    } else {
        Err(Error::new(ErrorKind::InvalidInput, "Path is invalid"))
    }
}

struct Httpd {
    root: PathBuf
}

impl Service for Httpd {
    type Request = Request;
    type Response = Response;
    type Error = hyper::Error;
    type Future = FutureResult<Response, hyper::Error>;
    fn call(&self, req: Request) -> Self::Future {
        let res = match *req.method() {
            Method::Get => {
                match read_req(&self.root, &req) {
                    Ok((headers, response)) => {
                        Response::new()
                            .with_headers(headers)
                            .with_body(response)
                    },
                    Err(err) => {
                        Response::new()
                            .with_status(match err.kind() {
                                ErrorKind::NotFound => StatusCode::NotFound,
                                ErrorKind::InvalidInput => StatusCode::BadRequest,
                                _ => StatusCode::InternalServerError
                            })
                            .with_body(format!("{}", err))
                    }
                }
            }
            _ => {
                Response::new()
                    .with_status(StatusCode::MethodNotAllowed)
            }
        };

        futures::future::ok(res)
    }
}

fn http(root: PathBuf) {
    let addr = "0.0.0.0:8080".parse().unwrap();
    let server = Http::new().bind(&addr, move || Ok(Httpd { root: root.clone() })).unwrap();
    server.run().unwrap();
}

#[cfg(target_os = "redox")]
fn fork()  -> usize {
    extern crate syscall;
    unsafe { syscall::clone(0).unwrap() }
}

#[cfg(not(target_os = "redox"))]
fn fork()  -> usize {
    extern crate libc;
    unsafe { libc::fork() as usize }
}

fn main() {
    let mut background = false;
    let mut root = env::current_dir().unwrap();
    for arg in env::args().skip(1) {
        match arg.as_ref() {
            "-b" => background = true,
            _ => root = fs::canonicalize(arg).unwrap()
        }
    }

    println!("HTTP: {}", root.display());
    if background {
        if fork() == 0 {
            http(root);
        }
    } else {
        http(root);
    }
}
