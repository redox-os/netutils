use std::io::{stdin, Read, Write};
use std::net::{TcpStream, TcpListener, UdpSocket};
use std::process::exit;
use std::str;
use std::thread;

macro_rules! print_err {
    ($($arg:tt)*) => (
        {
            use std::io::prelude::*;
            if let Err(e) = write!(&mut ::std::io::stderr(), "{}\n", format_args!($($arg)*)) {
                panic!("Failed to write to stderr.\
                    \nOriginal error output: {}\
                    \nSecondary error writing to stderr: {}", format!($($arg)*), e);
            }
        }
        )
}

// TODO: variable buffer size?
const BUFFER_SIZE: usize = 65636;

/// Read from the input file into a buffer in an infinite loop.
/// Handle the buffer content with handler function.
fn rw_loop<R, F>(input: &mut R, mut handler: F) -> ! 
    where R: Read, F: FnMut(&[u8], usize) -> ()
{
    loop {
        let mut buffer = [0u8; BUFFER_SIZE];
        // TODO: improve error messages
        let count  = match input.read(&mut buffer) {
            Ok(0) => {
                print_err!("End of input file/socket.");
                exit(0);
            }
            Ok(c) => c,
            Err(_) => {
                print_err!("Error occurred while reading from file/socket.");
                exit(1);
            }
        };
        handler(&buffer, count);
    }
}

/// Use the rw_loop in both direction (TCP connection)
fn both_dir_rw_loop(mut stream_read: TcpStream, mut stream_write: TcpStream) -> Result<(), String> {
    // Read loop
    thread::spawn(move || {
        rw_loop(&mut stream_read, |buffer, count| {
            print!("{}", unsafe { str::from_utf8_unchecked(&buffer[..count]) });
        });
    });

    // Write loop
    let mut stdin = stdin();
    rw_loop(&mut stdin, |buffer, count| {
        let _ = stream_write.write(&buffer[..count]).unwrap_or_else(|e| {
            print_err!("Error occurred while writing into socket: {} ", e);
            exit(1);
        });
    });
}

/// Connect to listening TCP socket
pub fn connect_tcp(host: &str) -> Result<(), String> {
    // Open socket and create its clone
    let stream_read = try!(TcpStream::connect(host)
                           .map_err(|e| {format!("connect_tcp error: can not create socket ({})", e)}));
    let stream_write = try!(stream_read.try_clone()
                            .map_err(|e| {format!("connect_tcp error: can not create socket clone ({})", e)}));

    print_err!("Remote host: {}", host);

    both_dir_rw_loop(stream_read, stream_write)

}

/// Listen on specified port and accept the first incoming connection
/// NOTE: "-k Accept multiple connections in listen mode" is not implemented
pub fn listen_tcp(host: &str) -> Result<(), String> {
    let listener = try!(TcpListener::bind(host)
                        .map_err(|e| {format!("connect_tcp error: can not bind to specified port ({})", e)}));
    let (stream_read, socketaddr) = try!(listener.accept()
                                         .map_err(|e| {format!("connect_tcp error: can not establish connection ({})", e)}));
    let stream_write = try!(stream_read.try_clone()
                            .map_err(|e| {format!("connect_tcp error: can not create socket clone ({})", e)}));
    print_err!("Incoming connection from: {}", socketaddr);
    both_dir_rw_loop(stream_read, stream_write)
}

/// Send UDP datagrams to specified socket
pub fn connect_udp(host: &str) -> Result<(), String> {
    // TODO: Implement some port selection process (while loop?)
    let socket = try!(UdpSocket::bind("localhost:30000")
                      .map_err(|e| {format!("connect_udp error: could not bind to local socket ({})", e)}));
    try!(socket.connect(host)
         .map_err(|e| {format!("connect_udp error: could not set up remote socket ({})", e)}));

    let mut stdin = stdin();
    rw_loop(&mut stdin, |buffer, count| {
        let _ = socket.send(&buffer[..count]).unwrap_or_else(|e| {
            print_err!("Error occurred while writing into socket: {} ", e);
            exit(1);
        });
    });
}

/// Listen for UDP datagrams on the specified socket
pub fn listen_udp(host: &str) -> Result<(), String> {
    let socket = try!(UdpSocket::bind(host)
                      .map_err(|e| {format!("connect_udp error: could not bind to local socket ({})", e)}));
    loop {
        let mut buffer = [0u8; BUFFER_SIZE];
        let count  = match socket.recv_from(&mut buffer) {
            Ok((0, _)) => {
                print_err!("End of input file/socket.");
                exit(0);
            }
            Ok((c, _)) => c,
            Err(_) => {
                print_err!("Error occurred while reading from file/socket.");
                exit(1);
            }
        };
        print!("{}", unsafe { str::from_utf8_unchecked(&buffer[..count]) });
    }
}

//TODO: write some unit tests
#[cfg(test)]
mod tests {

    #[test]
    fn pass() {
    }
}
