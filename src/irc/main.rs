use std::env;
use std::io::{stdin, Read, Write, Result};
use std::net::{TcpStream, ToSocketAddrs};
use std::str;
use std::sync::Arc;
use std::thread;

use std::cell::UnsafeCell;

/// Redox domain socket
pub struct Socket {
    file: UnsafeCell<TcpStream>
}

unsafe impl Send for Socket {}
unsafe impl Sync for Socket {}

impl Socket {
    pub fn connect<A: ToSocketAddrs>(addr: A) -> Result<Socket> {
        let file = try!(TcpStream::connect(addr));
        Ok(Socket {
            file: UnsafeCell::new(file)
        })
    }

    pub fn receive(&self, buf: &mut [u8]) -> Result<usize> {
        unsafe { (*self.file.get()).read(buf) }
    }

    pub fn send(&self, buf: &[u8]) -> Result<usize> {
        unsafe { (*self.file.get()).write(buf) }
    }
}


fn main() {
    let mut args = env::args().skip(1);

    let nick = args.next().expect("No nickname provided");

    let socket_write = Arc::new(Socket::connect("54.85.60.193:6667").expect("Failed to connect to irc.mozilla.org"));
    let socket_read = socket_write.clone();

    let register = format!("NICK {}\r\nUSER {} 0 * :{}\r\n", nick, nick, nick);
    print!("{}", register);
    socket_write.send(register.as_bytes()).unwrap();

    thread::spawn(move || {
        let mut chan_option = None;
        'stdin: loop {
            let mut line_original = String::new();
            if stdin().read_line(&mut line_original).unwrap() == 0 {
                println!("END OF INPUT");
                break 'stdin;
            }

            let line = line_original.trim();
            if line.starts_with('/') {
                let mut args = line.split(' ');
                if let Some(cmd) = args.next() {
                    match cmd {
                        "/msg" => if let Some(target) = args.next() {
                            let parts: Vec<&str> = args.collect();
                            let message = parts.join(" ");
                            socket_write.send(format!("PRIVMSG {} :{}\r\n", target, message).as_bytes()).unwrap();
                        } else {
                            println!("MSG: NO TARGET");
                        },
                        "/join" => if let Some(chan) = args.next() {
                            chan_option = Some(chan.to_string());
                            socket_write.send(format!("JOIN {}\r\n", chan).as_bytes()).unwrap();
                        } else {
                            println!("JOIN: NO CHANNEL");
                        },
                        "/leave" => if let Some(chan) = chan_option.take() {
                            socket_write.send(format!("PART {}\r\n", chan).as_bytes()).unwrap();
                        } else {
                            println!("LEAVE: NOT ON CHANNEL")
                        },
                        "/quit" => break 'stdin,
                        _ => println!("{}: UNKNOWN COMMAND", cmd)
                    }
                }
            } else if ! line.is_empty() {
                if let Some(ref chan) = chan_option {
                    socket_write.send(format!("PRIVMSG {} :{}\r\n", chan, line).as_bytes()).unwrap();
                } else {
                    println!("JOIN A CHANNEL");
                }
            }
        }

        socket_write.send(b"QUIT\r\n").unwrap();
    });

    'stdout: loop {
        let mut buffer = [0; 65536];
        let count = socket_read.receive(&mut buffer).unwrap();

        if count == 0 {
            println!("CONNECTION CLOSED");
            break 'stdout;
        }

        for line in unsafe { str::from_utf8_unchecked(&buffer[..count]) }.lines() {
            if line.starts_with("PING") {
                socket_read.send(format!("PONG {}\r\n", nick).as_bytes()).unwrap();
            } else {
                println!("{}", line);
            }
        }
    }
}
