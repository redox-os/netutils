extern crate syscall;
extern crate event;

use event::EventQueue;
use std::cell::RefCell;
use std::env::args;
use std::fs::File;
use std::io::{Read, Write};
use std::io::Error as IOError;
use std::num::ParseIntError;
use std::mem;
use std::net::{IpAddr, ToSocketAddrs};
use std::ops::{DerefMut, Deref};
use std::os::unix::io::{RawFd, FromRawFd};
use std::process;
use std::rc::Rc;
use std::slice;
use std::str::FromStr;
use syscall::data::TimeSpec;
use std::fmt;
use std::result;
use std::convert;

static PING_MAN: &'static str = /* @MANSTART{ping} */ r#"
NAME
    ping - send ICMP ECHO_REQUEST to network hosts

SYNOPSIS
    ping [-h | --help] [-c count] [-i interval] destination

DESCRIPTION
    ping sends ICMP ECHO_REQUEST packets to the specified destination host
    and reports on ECHO_RESPONSE packets it receives back.

OPTIONS
    -c count
        Number of packets to send. ping -c 0 will send packets until interrupted.

    -h
    --help
        Print this manual page.

    -i interval
        Wait interval seconds before sending next packet.
"#; /* @MANEND */

const PING_INTERVAL_S: i64 = 1;
const PING_TIMEOUT_S: i64 = 5;
const PING_PACKETS_TO_SEND: usize = 4;

enum ErrorType {
    IOError(IOError),
    IntParseError(ParseIntError),
    LocalError,
}

struct Error {
    error_type: ErrorType,
    descr: String,
}

impl Error {
    pub fn new_local<S: Into<String>>(descr: S) -> Error {
        Error {
            error_type: ErrorType::LocalError,
            descr: descr.into(),
        }
    }

    pub fn from_int_parse_error<S: Into<String>>(int_parse_error: ParseIntError,
                                                 descr: S)
                                                 -> Error {
        Error {
            error_type: ErrorType::IntParseError(int_parse_error),
            descr: descr.into(),
        }
    }

    pub fn from_io_error<S: Into<String>>(io_error: IOError, descr: S) -> Error {
        Error {
            error_type: ErrorType::IOError(io_error),
            descr: descr.into(),
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> result::Result<(), fmt::Error> {
        match self.error_type {
            ErrorType::IntParseError(ref int_parse_error) => {
                write!(f, "{} : int parse error: {}", self.descr, int_parse_error)
            }
            ErrorType::IOError(ref io_error) => {
                write!(f, "{} : io error : {}", self.descr, io_error)
            }
            ErrorType::LocalError => write!(f, "{}", self.descr),
        }
    }
}

impl convert::From<IOError> for Error {
    fn from(e: IOError) -> Self {
        Error::from_io_error(e, "")
    }
}

type Result<T> = result::Result<T, Error>;

#[repr(C)]
struct EchoPayload {
    seq: u16,
    timestamp: TimeSpec,
    payload: [u8; 40],
}

impl Deref for EchoPayload {
    type Target = [u8];
    fn deref(&self) -> &[u8] {
        unsafe {
            slice::from_raw_parts(self as *const EchoPayload as *const u8,
                                  mem::size_of::<EchoPayload>()) as &[u8]
        }
    }
}

impl DerefMut for EchoPayload {
    fn deref_mut(&mut self) -> &mut [u8] {
        unsafe {
            slice::from_raw_parts_mut(self as *mut EchoPayload as *mut u8,
                                      mem::size_of::<EchoPayload>()) as &mut [u8]
        }
    }
}

struct Ping {
    remote_host: IpAddr,
    time_file: File,
    echo_file: File,
    seq: usize,
    recieved: usize,
    //TODO: replace with BTreeMap once TimeSpec implements Ord
    waiting_for: Vec<(TimeSpec, usize)>,
    packets_to_send: usize,
    interval: i64,
}

fn time_diff_ms(from: &TimeSpec, to: &TimeSpec) -> f32 {
    ((to.tv_sec - from.tv_sec) * 1_000_000i64 +
     ((to.tv_nsec - from.tv_nsec) as i64) / 1_000i64) as f32 / 1_000.0f32
}

impl Ping {
    pub fn new(remote_host: IpAddr,
               packets_to_send: usize,
               interval: i64,
               echo_file: File,
               time_file: File)
               -> Ping {
        Ping {
            remote_host,
            echo_file,
            time_file,
            seq: 0,
            recieved: 0,
            waiting_for: vec![],
            packets_to_send,
            interval,
        }
    }

    pub fn on_echo_event(&mut self) -> Result<Option<()>> {
        let mut payload = EchoPayload {
            seq: 0,
            timestamp: TimeSpec::default(),
            payload: [0; 40],
        };
        let readed = match self.echo_file.read(&mut payload) {
            Ok(cnt) => cnt,
            Err(e) => {
                if e.raw_os_error() == Some(syscall::EAGAIN) {
                    0
                } else {
                    return Err(Error::from_io_error(e, "Failed to read from echo file"));
                }
            }
        };
        if readed == 0 {
            return Ok(None);
        }
        if readed < mem::size_of::<EchoPayload>() {
            return Err(Error::new_local("Not enough data in the echo file"));
        }
        let mut time = TimeSpec::default();
        syscall::clock_gettime(syscall::CLOCK_MONOTONIC, &mut time)
            .map_err(|_| Error::new_local("Failed to get the current time"))?;
        let remote_host = self.remote_host;
        let mut recieved = 0;
        self.waiting_for
            .retain(|&(_ts, seq)| if seq as u16 == payload.seq {
                        recieved += 1;
                        println!("From {} icmp_seq={} time={}ms",
                                 remote_host,
                                 seq,
                                 time_diff_ms(&payload.timestamp, &time));
                        false
                    } else {
                        true
                    });
        self.recieved += recieved;
        self.is_finished()
    }

    pub fn on_time_event(&mut self) -> Result<Option<()>> {
        let mut time = TimeSpec::default();
        if self.time_file.read(&mut time)? < mem::size_of::<TimeSpec>() {
            return Err(Error::new_local("Failed to read from time file"));
        }
        self.send_ping(&time)?;
        self.check_timeouts(&time)?;
        time.tv_sec += self.interval;
        self.time_file
            .write_all(&time)
            .map_err(|e| Error::from_io_error(e, "Failed to write to time file"))?;
        self.is_finished()
    }

    fn send_ping(&mut self, time: &TimeSpec) -> Result<Option<()>> {
        if self.packets_to_send != 0 && self.seq >= self.packets_to_send {
            return Ok(None);
        }
        let payload = EchoPayload {
            seq: self.seq as u16,
            timestamp: *time,
            payload: [1; 40],
        };
        let _ = self.echo_file.write(&payload)?;
        let mut timeout_time = *time;
        timeout_time.tv_sec += PING_TIMEOUT_S;
        self.waiting_for.push((timeout_time, self.seq));
        self.seq += 1;
        Ok(None)
    }

    fn check_timeouts(&mut self, time: &TimeSpec) -> Result<Option<()>> {
        let remote_host = self.remote_host;
        self.waiting_for
            .retain(|&(ts, seq)| if ts.tv_sec <= time.tv_sec {
                        println!("From {} icmp_seq={} timeout", remote_host, seq);
                        false
                    } else {
                        true
                    });
        Ok(None)
    }

    fn is_finished(&self) -> Result<Option<()>> {
        if self.packets_to_send != 0 && self.seq == self.packets_to_send &&
           self.waiting_for.is_empty() {
            Ok(Some(()))
        } else {
            Ok(None)
        }
    }

    fn get_transmitted(&self) -> usize {
        self.seq
    }

    fn get_recieved(&self) -> usize {
        self.recieved
    }
}

fn resolve_host(host: &str) -> Result<IpAddr> {
    match (host, 0).to_socket_addrs()?.next() {
        Some(addr) => Ok(addr.ip()),
        None => Err(Error::new_local("Failed to resolve remote host's IP address"))
    }
}

fn run() -> Result<()> {
    let mut args = args().skip(1);
    let mut count = PING_PACKETS_TO_SEND;
    let mut interval = PING_INTERVAL_S;
    let mut remote_host = "".to_owned();

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--help" | "-h" => {
                println!("{}", PING_MAN);
                return Ok(());
            }
            "-i" => {
                interval = i64::from_str(&args.next()
                                         .ok_or_else(|| {
                                             Error::new_local("No argument to -i option")
                                         })?)
                    .map_err(|e| {
                        Error::from_int_parse_error(e, "Invalid argument to -i option")
                    })?;
                if interval <= 0 {
                    return Err(Error::new_local("Interval can't be less or equal to 0"));
                }
            }
            "-c" => {
                count = usize::from_str(&args.next()
                                        .ok_or_else(|| {
                                            Error::new_local("No argument to -c option")
                                        })?)
                    .map_err(|e| {
                        Error::from_int_parse_error(e, "Invalid argument to -c option")
                    })?;
            }
            host => {
                if remote_host.is_empty() {
                    remote_host = host.to_owned();
                } else {
                    return Err(Error::new_local("Too many hosts to ping"));
                }
            }
        }
    }

    let remote_host = resolve_host(&remote_host)?;

    let icmp_path = format!("icmp:echo/{}", remote_host);
    let echo_fd = syscall::open(&icmp_path, syscall::O_RDWR | syscall::O_NONBLOCK)
        .map_err(|_| Error::new_local(format!("Can't open path {}", icmp_path)))?;

    let time_path = format!("time:{}", syscall::CLOCK_MONOTONIC);
    let time_fd = syscall::open(&time_path, syscall::O_RDWR)
        .map_err(|_| Error::new_local(format!("Can't open path {}", time_path)))?;


    let ping = Rc::new(RefCell::new(Ping::new(remote_host,
                                              count,
                                              interval,
                                              unsafe { File::from_raw_fd(echo_fd as RawFd) },
                                              unsafe { File::from_raw_fd(time_fd as RawFd) })));

    let mut event_queue =
        EventQueue::<(), Error>::new()
            .map_err(|e| Error::from_io_error(e, "Failed to create error queue"))?;

    let ping_ = ping.clone();

    event_queue
        .add(echo_fd as RawFd,
             move |_| ping_.borrow_mut().on_echo_event())?;

    let ping_ = ping.clone();
    event_queue
        .add(time_fd as RawFd,
             move |_| ping_.borrow_mut().on_time_event())?;

    event_queue.trigger_all(event::Event {
        fd: 0,
        flags: syscall::EventFlags::empty(),
    })?;

    event_queue.run()?;

    let transmited = ping.borrow().get_transmitted();
    let recieved = ping.borrow().get_recieved();
    println!("--- {} ping statistics ---", remote_host);
    println!("{} packets transmitted, {} recieved, {}% packet loss",
             transmited,
             recieved,
             100 * (transmited - recieved) / transmited);
    Ok(())
}

fn main() {
    if let Err(err) = run() {
        println!("{}", err);
        process::exit(1);
    }
}
