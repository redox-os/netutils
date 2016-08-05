use std::io::{stdin, Read, Write};
use std::net::TcpStream;
use std::process::exit;
use std::str;
use std::thread;

// TODO: variable buffer size? 
const BUFFER_SIZE: usize = 65636;

/// Connect to listening TCP socket
pub fn connect_tcp(host: String) -> Result<(), String> {
    // Open socket and create its clone
    let mut stream_read = try!(TcpStream::connect(host.as_str())
        .map_err(|e| {format!("connect_tcp error: can not create socket ({})", e)}));
    let mut stream_write = try!(stream_read.try_clone()
        .map_err(|e| {format!("connect_tcp error: can not create socket clone ({})", e)}));

    println!("Remote host: {}", host);

    // Read loop
    thread::spawn(move || {
        loop {
            let mut buffer = [0u8; BUFFER_SIZE];
            // TODO: improve error messages
            let count  = match stream_read.read(&mut buffer) {
                Ok(0) => {
                    println!("End of input file.");
                    exit(0);
                }
                Ok(c) => c,
                Err(_) => {
                    println!("Error occurred while reading from socket.");
                    exit(1);
                }
            };
            print!("{}", unsafe { str::from_utf8_unchecked(&buffer[..count]) });
        }
    });

    // Write loop
    loop {
        let mut buffer = [0; BUFFER_SIZE];
        let count = match stdin().read(&mut buffer) {
            Ok(0) => {
                println!("End of input file.");
                exit(0);
            }
            Ok(c) => c,
            Err(_) => {
                println!("Error occured while reading from stdin.");
                exit(1);
            }
        };
        let _ = stream_write.write(&buffer[..count]).unwrap_or_else(|e| {
            println!("Error occurred while writing into socket.");
            exit(1);
        });
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
