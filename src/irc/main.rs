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

    let socket_write = Arc::new(Socket::connect("irc.mozilla.org:6667").expect("Failed to connect to irc.mozilla.org"));
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
                            println!("irc: MSG: No message target given, use /msg target_user message.");
                        },
                        "/join" => if let Some(ref chan) = chan_option {
                            println!("irc: JOIN: You already are on {}.", chan);
                        } else {
                            if let Some(chan) = args.next() {
                                chan_option = Some(chan.to_string());
                                socket_write.send(format!("JOIN {}\r\n", chan).as_bytes()).unwrap();
                            } else {
                                println!("irc: JOIN: You must provide a channel to join, use /join #chan_name.");
                            }
                        },
                        "/leave" => if let Some(chan) = chan_option.take() {
                            socket_write.send(format!("PART {}\r\n", chan).as_bytes()).unwrap();
                        } else {
                            println!("irc: LEAVE: You aren't connected to any channels.")
                        },
                        "/quit" => break 'stdin,
                        _ => println!("irc: {}: Unknown command.", cmd)
                    }
                }
            } else if ! line.is_empty() {
                if let Some(ref chan) = chan_option {
                    socket_write.send(format!("PRIVMSG {} :{}\r\n", chan, line).as_bytes()).unwrap();
                } else {
                    println!("irc: You haven't joined a channel yet, use /join #chan_name");
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
            let mut args = line.split(' ');

            let prefix = if line.starts_with(':') {
                args.next()
            } else {
                None
            };

            let source = prefix.unwrap_or("").split(':').nth(1).unwrap_or("").split("!").next().unwrap_or("");

            if let Some(cmd) = args.next() {
                match cmd {
                    "ERROR" => {
                        let parts: Vec<&str> = args.collect();
                        let mut message = parts.join(" ");
                        if message.starts_with(':') {
                            message.remove(0);
                        }
                        println!("\x1B[1mERROR: {}\x1B[21m", message);
                    },
                    "JOIN" => {
                        let parts: Vec<&str> = args.collect();
                        let mut message = parts.join(" ");
                        if message.starts_with(':') {
                            message.remove(0);
                        }
                        println!("\x1B[1m{} joined {}\x1B[21m", source, message);
                    },
                    "MODE" => {
                        let target = args.next().unwrap_or("");
                        let mode = args.next().unwrap_or("");
                        println!("\x1B[1m{} set to mode {}\x1B[21m", target, mode);
                    },
                    "NOTICE" => {
                        let _target = args.next().unwrap_or("");
                        let parts: Vec<&str> = args.collect();
                        let mut message = parts.join(" ");
                        if message.starts_with(':') {
                            message.remove(0);
                        }
                        println!("\x1B[7m\x1B[1m{}: {}\x1B[21m\x1B[27m", source, message);
                    },
                    "PART" => {
                        let parts: Vec<&str> = args.collect();
                        let mut message = parts.join(" ");
                        if message.starts_with(':') {
                            message.remove(0);
                        }
                        println!("\x1B[1m{} parted {}\x1B[21m", source, message);
                    },
                    "PING" => {
                        socket_read.send(format!("PONG {}\r\n", nick).as_bytes()).unwrap();
                    },
                    "PRIVMSG" => {
                        let _target = args.next().unwrap_or("");
                        let parts: Vec<&str> = args.collect();
                        let mut message = parts.join(" ");
                        if message.starts_with(':') {
                            message.remove(0);
                        }
                        println!("\x1B[7m{}: {}\x1B[27m", source, message);
                    },
                    "QUIT" => {
                        let parts: Vec<&str> = args.collect();
                        let mut message = parts.join(" ");
                        if message.starts_with(':') {
                            message.remove(0);
                        }
                        println!("\x1B[1m{} quit: {}\x1B[21m", source, message);
                    },
                    "372" => {
                        let _target = args.next().unwrap_or("");
                        let parts: Vec<&str> = args.collect();
                        let mut message = parts.join(" ");
                        if message.starts_with(':') {
                            message.remove(0);
                        }
                        println!("\x1B[1m{}\x1B[21m", message);
                    },
                    _ => {
                        println!("{}", line);
                    }
                }
            }
        }
    }
}
