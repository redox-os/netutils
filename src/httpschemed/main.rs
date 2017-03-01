extern crate hyper;
extern crate hyper_rustls;
extern crate spin;
extern crate syscall;

use std::fs::File;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::io::{Read, Write};
use std::str;

use syscall::error::{Error, Result, EBADF, ENOENT};
use syscall::{Packet, SchemeMut};

use hyper::Client;
use hyper::net::HttpsConnector;
use hyper::client::response::Response;

use spin::Mutex;


pub struct HttpScheme {
    client: Client,
    responses: Mutex<BTreeMap<usize, Box<Response>>>,
    next_id: AtomicUsize
}

impl HttpScheme {
    pub fn new() -> HttpScheme {
        HttpScheme {
            client: Client::with_connector(HttpsConnector::new(hyper_rustls::TlsClient::new())),
            responses: Mutex::new(BTreeMap::new()),
            next_id: AtomicUsize::new(1)
        }
    }
}

impl SchemeMut for HttpScheme {
    fn open(&mut self, path: &[u8], _flags: usize, _uid: u32, _gid: u32) -> Result<usize> {
        match str::from_utf8(path) {
            Ok(path) => {
                let mut url = String::from("http://");
                url.push_str(path);

                match self.client.get(&url).send() {
                    Ok(res) => {
                        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
                        self.responses.lock().insert(id, Box::new(res));
                        Ok(id)
                    }
                    Err(_) => Err(Error::new(ENOENT)) // TODO: Handle specific error types
                }
            },
            Err(_) => Err(Error::new(ENOENT)) // TODO: Handle specific error types
        }
    }

    fn read(&mut self, id: usize, buf: &mut [u8]) -> Result<usize> {
        let mut responses = self.responses.lock();
        if let Some(mut res) = responses.get_mut(&id) {
            match res.read(buf) {
                Ok(num) => Ok(num),
                Err(_) => Err(Error::new(EBADF)) // TODO: Handle specific error types
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
    let mut socket = File::create(":http").expect("http: failed to create http scheme");
    let mut scheme = HttpScheme::new();

    loop {
        let mut packet = Packet::default();
        socket.read(&mut packet).expect("http: failed to read events from http scheme");
        scheme.handle(&mut packet);
        socket.write(&packet).expect("http: failed to write responses to http scheme");
    }
}
