use std::env;
use std::io::{stdin, Read, Write, Result};
use std::net::{TcpStream, ToSocketAddrs};
use std::str;
use std::sync::{Arc, Mutex};
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

/// Channel struct used to store currently open channels,
/// and a buffer of messages received when the channel
/// wasn't focused on
#[derive(Clone)]
pub struct Channel {
    pub name: String,
    pub buffer: String,
    pub unread: u32,
}

impl Channel {
    fn new(name: String) -> Self {
        Channel {
            name: name,
            buffer: String::new(),
            unread: 0,
        }
    }

    fn get_name(&self) -> String {
        self.name.clone()
    }

    fn push(&mut self, arg: &str) {
        self.buffer.push_str(arg);
    }

    fn dump_buf(&mut self) {
        print!("{}", self.buffer);
        self.buffer = String::new();
        self.unread = 0;
    }
}


fn main() {
    use std::num::Wrapping;

    let mut args = env::args().skip(1);

    let nick = args.next().expect("No nickname provided");

    let socket_write = Arc::new(Socket::connect("irc.mozilla.org:6667").expect("Failed to connect to irc.mozilla.org"));
    let socket_read = socket_write.clone();

    let channels: Arc<Mutex<(Vec<Channel>, Wrapping<usize>)>> = Arc::new(Mutex::new((vec![], Wrapping(0))));
    let channels_thread = channels.clone(); // Reference sent out to the thread

    let register = format!("NICK {}\r\nUSER {} 0 * :{}\r\n", nick, nick, nick);
    print!("{}", register);
    socket_write.send(register.as_bytes()).unwrap();

    thread::spawn(move || {
        let channels = channels_thread;
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
                        "/join" => {
                            if let Some(chan) = args.next() {
                                let channel = Channel::new(chan.to_string());
                                let mut channels_lock = channels.lock().unwrap();

                                channels_lock.0.push(channel);
                                channels_lock.1 = Wrapping(channels_lock.0.len() - 1);
                                socket_write.send(format!("JOIN {}\r\n", chan).as_bytes()).unwrap();
                            } else {
                                println!("irc: JOIN: You must provide a channel to join, use /join #chan_name.");
                            }
                        },
                        "/next" => {
                            let mut channels_lock = channels.lock().unwrap();

                            channels_lock.1 += Wrapping(1);
                            channels_lock.1 %= Wrapping(channels_lock.0.len());
                            println!("irc: Talking on {}", channels_lock.0.get((channels_lock.1).0).unwrap().name);
                            let channel_number = (channels_lock.1).0;
                            channels_lock.0.get_mut(channel_number).unwrap().dump_buf();
                        },
                        "/back" => {
                            let mut channels_lock = channels.lock().unwrap();

                            channels_lock.1 -= Wrapping(1);
                            channels_lock.1 %= Wrapping(channels_lock.0.len());       
                            println!("irc: Talking on {}", channels_lock.0.get((channels_lock.1).0).unwrap().name);  
                            let channel_number = (channels_lock.1).0;     
                            channels_lock.0.get_mut(channel_number).unwrap().dump_buf();             
                        },
                        "/goto" => {
                            let mut channels_lock = channels.lock().unwrap();

                            if let Some(n) = args.next() {
                                let n = n.parse::<usize>();
                                if n.is_err() {
                                    println!("irc: GOTO: You must provide the channel's number. You can find the number by using /list");
                                } else {
                                    let n = n.unwrap();
                                    if n < 1 || n > channels_lock.0.len() + 1 {
                                        println!("irc: GOTO: This channel number is invalid. You can find the number by using /list");
                                    } else {
                                        channels_lock.1 = Wrapping(n - 1);
                                        // Leaving this just in case, remove if you want to, this protects from accidentaly setting a wrong
                                        // channel ID
                                        channels_lock.1 %= Wrapping(channels_lock.0.len());       
                                        println!("irc: Talking on {}", channels_lock.0.get((channels_lock.1).0).unwrap().name);
                                    }   
                                }
                            } else {
                                println!("irc: GOTO: You must provide the channel's number. You can find it by using /list");
                            }
                        }
                        "/list" => {
                            let mut channels_lock = channels.lock().unwrap();
                            println!("irc: Currently connected to:");
                            for (i, channel) in channels_lock.0.iter().enumerate() {
                                if i == (channels_lock.1).0 {
                                    println!("{}. > {}", i + 1, channel.get_name());
                                } else { 
                                    println!("{}.   {}, {} unread", i + 1, channel.get_name(), channel.unread);
                                }
                            }
                        },
                        "/leave" | "/part" => { 
                            let mut channels_lock = channels.lock().unwrap();

                            if channels_lock.0.get((channels_lock.1).0).is_some() {
                                {
                                    let chan = channels_lock.0.get((channels_lock.1).0).unwrap().get_name();
                                    socket_write.send(format!("PART {}\r\n", chan).as_bytes()).unwrap();
                                }
                                let channel_number = (channels_lock.1).0;

                                channels_lock.0.remove(channel_number);
                                (channels_lock.1).0 = channel_number - 1;
                            } else {
                                println!("irc: LEAVE: You aren't connected to any channels.")
                            }
                        },
                        "/help" | "/commands" => {
                            println!("irc: Available commands:");
                            println!("     /join <channel_name> - Joins a channel");
                            println!("     /list - Lists channels you're connected to");
                            println!("     /next - Goes to the next channel");
                            println!("     /back - Goes to the earlier channel");
                            println!("     /goto <channel_number> - Goes to a specified channel");
                            println!("     /msg <user> <message> - Sends a private message");
                            println!("     /leave or /part - Leaves a channel");
                            println!("     /quit or /exit - Exits this program");
                            println!("     /help or /commands - Shows this help message");
                        }
                        "/quit" | "/exit" => break 'stdin,
                        _ => println!("irc: {}: Unknown command. Try /help", cmd)
                    }
                }
            } else if ! line.is_empty() {
                let channels_lock = channels.lock().unwrap();

                if let Some(ref chan) = channels_lock.0.get((channels_lock.1).0) {
                    socket_write.send(format!("PRIVMSG {} :{}\r\n", chan.name, line).as_bytes()).unwrap();
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
                        let mut channels_lock = channels.lock().unwrap();

                        let _target = args.next().unwrap_or("");

                        let channel: Option<&mut Channel>;
                        channel = channels_lock.0.iter_mut().filter(|chan| {
                            chan.get_name() == _target
                        }).next(); 

                        let parts: Vec<&str> = args.collect();
                        let mut message = parts.join(" ");
                        if message.starts_with(':') {
                            message.remove(0);
                        }

                        if channel.is_some(){
                            let mut channel = channel.unwrap();
                            //println!("Message hidden"); // this for testing
                            channel.buffer.push_str(&format!("\x1B[7m{} {}: {}\x1B[27m\n", _target, source, message));
                            channel.unread += 1;             
                        } else {
                            println!("\x1B[7m{} {}: {}\x1B[27m", _target, source, message);
                        }
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

        let mut channels_lock = channels.lock().unwrap();
        let channel_number = (channels_lock.1).0;
        let mut channel: Option<&mut Channel> = channels_lock.0.get_mut(channel_number);
        if channel.is_some() {
            let mut channel: &mut Channel = channel.unwrap();
            channel.dump_buf();
        }
    }
}
