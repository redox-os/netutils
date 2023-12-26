extern crate anyhow;
extern crate event;
extern crate libredox;

use std::borrow::BorrowMut;
use std::env::args;
use std::io::Error as IOError;
use std::mem;
use std::net::{IpAddr, ToSocketAddrs};
use std::ops::{DerefMut, Deref};
use std::process;
use std::slice;
use std::str::FromStr;

use anyhow::{bail, Error, Result, Context, anyhow};
use event::{EventQueue, user_data, EventFlags};
use libredox::{Fd, flag};
use libredox::data::TimeSpec;

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
    time_file: Fd,
    echo_file: Fd,
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
               echo_file: Fd,
               time_file: Fd)
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
            timestamp: TimeSpec { tv_sec: 0, tv_nsec: 0 },
            payload: [0; 40],
        };
        let readed = match self.echo_file.read(&mut payload) {
            Ok(cnt) => cnt,
            Err(e) if e.is_wouldblock() => {
                0
            }
            Err(e) => return Err(e).context("Failed to read from echo file"),
        };
        if readed == 0 {
            return Ok(None);
        }
        if readed < mem::size_of::<EchoPayload>() {
            bail!("Not enough data in the echo file");
        }
        let time = libredox::call::clock_gettime(libredox::flag::CLOCK_MONOTONIC)
            .context("Failed to get the current time")?;
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
        let mut buf = [0_u8; mem::size_of::<TimeSpec>()];
        if self.time_file.read(&mut buf)? < mem::size_of::<TimeSpec>() {
            bail!("Failed to read from time file");
        }
        let time = libredox::data::timespec_from_mut_bytes(&mut buf);
        self.send_ping(&time)?;
        self.check_timeouts(&time)?;
        time.tv_sec += self.interval;
        self.time_file
            .write(&buf)
            .context("Failed to write to time file")?;
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
        None => Err(anyhow!("Failed to resolve remote host's IP address")),
    }
}

fn main() -> Result<()> {
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
                                             anyhow!("No argument to -i option")
                                         })?)
                    .map_err(|e| {
                        anyhow!("{e}: Invalid argument to -i option")
                    })?;
                if interval <= 0 {
                    bail!("Interval can't be less or equal to 0");
                }
            }
            "-c" => {
                count = usize::from_str(&args.next()
                                        .ok_or_else(|| {
                                            anyhow!("No argument to -c option")
                                        })?)
                    .map_err(|e| {
                        anyhow!("{e}: Invalid argument to -c option")
                    })?;
            }
            host => {
                if remote_host.is_empty() {
                    remote_host = host.to_owned();
                } else {
                    bail!("Too many hosts to ping");
                }
            }
        }
    }

    let remote_host = resolve_host(&remote_host)?;

    let icmp_path = format!("icmp:echo/{}", remote_host);
    let echo_fd = Fd::open(&icmp_path, flag::O_RDWR | flag::O_NONBLOCK, 0)
        .map_err(|_| anyhow!("Can't open path {}", icmp_path))?;

    let time_path = format!("time:{}", flag::CLOCK_MONOTONIC);
    let time_fd = Fd::open(&time_path, flag::O_RDWR, 0)
        .map_err(|_| anyhow!("Can't open path {}", time_path))?;

    user_data! {
        enum EventSource {
            Echo,
            Time,
        }
    }

    let event_queue = EventQueue::<EventSource>::new()
        .context("Failed to create error queue")?;

    event_queue.subscribe(echo_fd.raw(), EventSource::Echo, EventFlags::READ)?;
    event_queue.subscribe(time_fd.raw(), EventSource::Time, EventFlags::READ)?;

    let mut ping = Ping::new(remote_host, count, interval, echo_fd, time_fd);

    let _ = ping.on_echo_event();
    let _ = ping.on_time_event();

    for event_res in event_queue {
        let _ = match event_res?.user_data {
            EventSource::Echo => ping.on_echo_event(),
            EventSource::Time => ping.on_time_event(),
        };
    }

    let transmited = ping.get_transmitted();
    let recieved = ping.get_recieved();
    println!("--- {} ping statistics ---", remote_host);
    println!("{} packets transmitted, {} recieved, {}% packet loss",
             transmited,
             recieved,
             100 * (transmited - recieved) / transmited);
    Ok(())
}
