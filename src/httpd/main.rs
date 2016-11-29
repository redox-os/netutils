#![cfg_attr(not(target_os = "redox"), feature(libc))]

use std::{env, str};
use std::fs::{self, File};
use std::io::{Error, ErrorKind, Result, Read, Write};
use std::net::TcpListener;
use std::path::Path;

fn read_dir(root: &Path, path: &Path) -> Result<(Vec<u8>, Vec<u8>)> {
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

    let headers = format!("Content-Type: text/html\r\n").into_bytes();

    Ok((headers, response.into_bytes()))
}

fn read_file(_root: &Path, path: &Path) -> Result<(Vec<u8>, Vec<u8>)> {
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

    let headers = format!("Content-Type: {}\r\n", mime_type).into_bytes();

    Ok((headers, response))
}

fn read_path(root: &Path, path: &Path) -> Result<(Vec<u8>, Vec<u8>)> {
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

fn read_req(root: &Path, request: &str) -> Result<(Vec<u8>, Vec<u8>)> {
    let get = request.lines().next().ok_or(Error::new(ErrorKind::InvalidInput, "Request line not found"))?;
    let path = get.split(' ').nth(1).ok_or(Error::new(ErrorKind::InvalidInput, "Path not found"))?;

    let mut full_path = root.to_path_buf();
    full_path.push(path.trim_left_matches('/'));
    if full_path.as_path().strip_prefix(root).is_ok() {
        read_path(root, &full_path)
    } else {
        Err(Error::new(ErrorKind::InvalidInput, "Path is invalid"))
    }
}

fn http(root: &Path) {
    let listener = TcpListener::bind("0.0.0.0:8080").unwrap();
    loop {
        let mut stream = listener.accept().unwrap().0;

        let mut data = [0; 65536];
        let count = stream.read(&mut data).unwrap();

        let request = str::from_utf8(&data[.. count]).unwrap();

        let response = match read_req(root, request) {
            Ok((mut headers, mut response)) => {
                let mut full_response = format!("HTTP/1.1 200 OK\r\n").into_bytes();
                full_response.append(&mut headers);
                full_response.push(b'\r');
                full_response.push(b'\n');
                full_response.append(&mut response);
                full_response
            },
            Err(err) => match err.kind() {
                ErrorKind::NotFound => format!("HTTP/1.1 404 Not Found\r\n\r\n{}", err).into_bytes(),
                ErrorKind::InvalidInput => format!("HTTP/1.1 400 Bad Request\r\n\r\n{}", err).into_bytes(),
                _ => format!("HTTP/1.1 500 Internal Server Error\r\n\r\n{}", err).into_bytes()
            }
        };

        for chunk in response.chunks(8192) {
            stream.write(&chunk).unwrap();
        }
    }
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
            http(&root);
        }
    } else {
        http(&root);
    }
}
