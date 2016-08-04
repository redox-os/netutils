use std::io::{stdin, Read, Write};
use std::net::TcpStream;
use std::process::exit;
use std::str;
use std::thread;

// TODO: variable buffer size? 
const BUFFER_SIZE: usize = 65636;

pub fn connect_tcp(host: String) -> Result<(), String> {
    let mut stream_read = try!(TcpStream::connect(host.as_str())
        .map_err(|e| {format!("connect_tcp error: can not create socket ({})", e)}));
    let mut stream_write = try!(stream_read.try_clone()
        .map_err(|e| {format!("connect_tcp error: can not create socket clone ({})", e)}));

    println!("Remote host: {}", host);

    thread::spawn(move || {
        loop {
            let mut buffer = [0u8; BUFFER_SIZE];
            // TODO: improve error handling
            let count  = match stream_read.read(&mut buffer) {
                Ok(c) => {
                    // TODO: this should go out
                    if c == 0 {
                        println!("Connection closed");
                        exit(0);
                    }
                    c
                }
                Err(_) => {
                    println!("Error occured while reading from socket.");
                    exit(1);
                }
            };
            print!("{}", unsafe { str::from_utf8_unchecked(&buffer[..count]) });
        }
    });

    loop {
        let mut buffer = [0; BUFFER_SIZE];
        let count = stdin().read(&mut buffer).unwrap();
        let _ = stream_write.write(&buffer[..count]).unwrap();
    }
}

pub fn listen_tcp(host: String) -> Result<(), String> {
    println!("Not implemented");
    Ok(())
}


//TODO: write some unit tests
#[cfg(test)]
mod tests {

    #[test]
    fn pass() {
    }
}
