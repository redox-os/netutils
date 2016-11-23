use std::{env, str};
use std::fs::{self, File};
use std::io::{Error, ErrorKind, Result, Read, Write};
use std::net::TcpListener;
use std::path::Path;

fn read_dir(root: &Path, path: &Path) -> Result<Vec<u8>> {
    let mut names = vec![];
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        if let Some(name) = entry.file_name().to_str() {
            names.push(name.to_string());
        }
    }
    names.sort();

    let mut response = String::new();
    response.push_str("<!DOCTYPE html>\n<html>\n<body>\n");
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
    response.push_str("</body>\n</html>\n");

    Ok(response.into_bytes())
}

fn read_file(_root: &Path, path: &Path) -> Result<Vec<u8>> {
    let mut response = Vec::new();

    let mut file = File::open(path)?;
    file.read_to_end(&mut response)?;

    Ok(response)
}

fn read_path(root: &Path, path: &Path) -> Result<Vec<u8>> {
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

fn read_req(root: &Path, request: &str) -> Result<Vec<u8>> {
    let get = request.lines().next().ok_or(Error::new(ErrorKind::InvalidInput, "Request line not found"))?;
    let path = get.split(' ').nth(1).ok_or(Error::new(ErrorKind::InvalidInput, "Path not found"))?;

    if ! path.starts_with('/') || path.contains("..") {
        return Err(Error::new(ErrorKind::InvalidInput, "Path is invalid"));
    }

    let mut full_path = root.to_path_buf();
    full_path.push(path.trim_left_matches('/'));
    read_path(root, &full_path)
}

fn http(root: &Path) {
    let listener = TcpListener::bind("0.0.0.0:8080").unwrap();
    loop {
        let mut stream = listener.accept().unwrap().0;

        let mut data = [0; 65536];
        let count = stream.read(&mut data).unwrap();

        let request = str::from_utf8(&data[.. count]).unwrap();

        let response = match read_req(root, request) {
            Ok(mut response) => {
                let mut header = format!("HTTP/1.1 200 OK\r\n\r\n").into_bytes();
                header.append(&mut response);
                header
            }
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

fn main() {
    let mut background = false;
    let mut root = env::current_dir().unwrap();
    for arg in env::args().skip(1) {
        match arg.as_ref() {
            "-b" => background = true,
            _ => root = fs::canonicalize(arg).unwrap()
        }
    }

    println!("Root {}", root.display());
    if background {
        //if unsafe { syscall::clone(0).unwrap() } == 0
        {
            http(&root);
        }
    } else {
        http(&root);
    }
}
