extern crate hyper;
extern crate hyper_rustls;
extern crate spin;
extern crate syscall;

use std::fs::File;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::io::{Read, Write};
use std::str;

use syscall::error::{Error, Result, EBADF, ENOENT, EACCES, EINVAL, EIO, EPROTO};
use syscall::{Packet, SchemeMut};

use hyper::Client;
use hyper::net::HttpsConnector;
use hyper::client::response::Response;
use hyper::status::StatusCode;
use hyper::error::Error as HyperError;

use spin::Mutex;


struct HttpScheme {
    client: Client,
    responses: Mutex<BTreeMap<usize, Box<Response>>>,
    next_id: AtomicUsize,
    prefix: String
}

impl HttpScheme {
    pub fn new(scheme: &str) -> HttpScheme {
        let mut prefix = String::from(scheme);
        prefix.push_str("://");

        HttpScheme {
            client: Client::with_connector(HttpsConnector::new(hyper_rustls::TlsClient::new())),
            responses: Mutex::new(BTreeMap::new()),
            next_id: AtomicUsize::new(1),
            prefix: prefix
        }
    }
}

impl SchemeMut for HttpScheme {
    fn open(&mut self, path: &[u8], _flags: usize, _uid: u32, _gid: u32) -> Result<usize> {
        let path = str::from_utf8(path).or(Err(Error::new(EINVAL)))?;

        let mut url = self.prefix.clone();
        url.push_str(path);

        match self.client.get(&url).send() {
            Ok(res) => {
                match res.status {
                    StatusCode::Ok => {
                        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
                        self.responses.lock().insert(id, Box::new(res));
                        Ok(id)
                    }
                    StatusCode::NotFound => Err(Error::new(ENOENT)),
                    StatusCode::Forbidden => Err(Error::new(EACCES)),
                    // TODO: Handle more
                    _ => Err(Error::new(ENOENT))
                }
            }
            Err(err) => Err(Error::new(match err {
                HyperError::Uri(_) | HyperError::Utf8(_) => EINVAL,
                HyperError::Io(_) => EIO,
                // TODO: Handle more
                _ => EPROTO
            }))
        }
    }

    fn read(&mut self, id: usize, buf: &mut [u8]) -> Result<usize> {
        let mut responses = self.responses.lock();
        if let Some(mut res) = responses.get_mut(&id) {
            match res.read(buf) {
                Ok(num) => Ok(num),
                Err(_) => Err(Error::new(EIO))
            }
        } else {
            Err(Error::new(EBADF))
        }
    }

    fn close(&mut self, id: usize) -> Result<usize> {
        let mut responses = self.responses.lock();
        if responses.remove(&id).is_some() {
            Ok(0)
        } else {
            Err(Error::new(EBADF))
        }
    }
}


fn main() {
    let prot = std::env::args().nth(1).unwrap();

    // Daemonize
    if unsafe { syscall::clone(0).unwrap() } == 0 {
        let mut socket = File::create(format!(":{}", &prot))
            .expect(&format!("hyperd: failed to create {} scheme", prot));
        let mut scheme = HttpScheme::new(&prot);

        loop {
            let mut packet = Packet::default();
            socket.read(&mut packet)
                .expect(&format!("hyperd: failed to read events from {} scheme", prot));
            scheme.handle(&mut packet);
            socket.write(&packet)
                .expect(&format!("hyperd: failed to write responses to {} scheme", prot));
        }
    }
}
