extern crate anyhow;
extern crate event;
extern crate libredox;

use std::env::args;
use std::mem;
use std::net::{IpAddr, ToSocketAddrs};
use std::ops::{Deref, DerefMut};
use std::slice;
use std::str::FromStr;

use std::cmp::{Ordering, PartialOrd};
use std::collections::BTreeMap;

use std::fmt;

use anyhow::{anyhow, bail, Context, Result};
use event::{user_data, EventFlags, EventQueue};
use libredox::data::TimeSpec;
use libredox::errno::EINTR;
use libredox::{flag, Fd};

static PING_MAN: &'static str = /* @MANSTART{ping} */
    r#"
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
const ECHO_PAYLOAD_SIZE: usize = 40;
const IP_HEADER_SIZE: usize = 20;
const ICMP_HEADER_SIZE: usize = 8;

#[derive(Clone, Copy)]
struct OrderedTimeSpec(libredox::data::TimeSpec);

impl PartialEq for OrderedTimeSpec {
    fn eq(&self, other: &Self) -> bool {
        self.0.tv_sec == other.0.tv_sec && self.0.tv_nsec == other.0.tv_nsec
    }
}

impl fmt::Debug for OrderedTimeSpec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "OrderedTimeSpec {{ tv_sec: {}, tv_nsec: {} }}", self.0.tv_sec, self.0.tv_nsec)
    }
}

impl Eq for OrderedTimeSpec {}

impl PartialOrd for OrderedTimeSpec {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        let sec_order = self.0.tv_sec.cmp(&other.0.tv_sec);
        if sec_order == Ordering::Equal {
            Some(self.0.tv_nsec.cmp(&other.0.tv_nsec))
        } else {
            Some(sec_order)
        }
    }
}

impl Ord for OrderedTimeSpec {
    fn cmp(&self, other: &Self) -> Ordering {
        self.partial_cmp(other).unwrap()
    }
}

#[repr(C)]
struct EchoPayload {
    seq: u16,
    timestamp: TimeSpec,
    payload: [u8; ECHO_PAYLOAD_SIZE],
}


impl Deref for EchoPayload {
    type Target = [u8];
    fn deref(&self) -> &[u8] {
        unsafe {
            slice::from_raw_parts(
                self as *const EchoPayload as *const u8,
                mem::size_of::<EchoPayload>(),
            ) as &[u8]
        }
    }
}

impl DerefMut for EchoPayload {
    fn deref_mut(&mut self) -> &mut [u8] {
        unsafe {
            slice::from_raw_parts_mut(
                self as *mut EchoPayload as *mut u8,
                mem::size_of::<EchoPayload>(),
            ) as &mut [u8]
        }
    }
}

struct Ping {
    remote_host: IpAddr,
    time_file: Fd,
    echo_file: Fd,
    seq: usize,
    recieved: usize,
    //Replace Vec with BTreeMap
    waiting_for: BTreeMap<OrderedTimeSpec, usize>,
    packets_to_send: usize,
    interval: i64,
}

fn time_diff_ms(from: &TimeSpec, to: &TimeSpec) -> f32 {
    ((to.tv_sec - from.tv_sec) * 1_000_000i64 + ((to.tv_nsec - from.tv_nsec) as i64) / 1_000i64)
        as f32
        / 1_000.0f32
}

impl Ping {
    pub fn new(
        remote_host: IpAddr,
        packets_to_send: usize,
        interval: i64,
        echo_file: Fd,
        time_file: Fd,
    ) -> Ping {
        Ping {
            remote_host,
            echo_file,
            time_file,
            seq: 0,
            recieved: 0,
            // Initialize as BTreeMap
            waiting_for: BTreeMap::new(),
            packets_to_send,
            interval,
        }
    }

    pub fn on_echo_event(&mut self) -> Result<Option<()>> {
        let mut payload = EchoPayload {
            seq: 0,
            timestamp: TimeSpec {
                tv_sec: 0,
                tv_nsec: 0,
            },
            payload: [0; ECHO_PAYLOAD_SIZE],
        };
        let readed = match self.echo_file.read(&mut payload) {
            Ok(cnt) => cnt,
            Err(e) if e.is_wouldblock() => 0,
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
        self.waiting_for.retain(|_ts, &mut seq| {
            if seq as u16 == payload.seq {
                recieved += 1;
                println!(
                    "From {} icmp_seq={} time={}ms",
                    remote_host,
                    seq,
                    time_diff_ms(&payload.timestamp, &time)
                );
                false
            } else {
                true
            }
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
            payload: [1; ECHO_PAYLOAD_SIZE],
        };

        let _ = self.echo_file.write(&payload)?;

        let mut timeout_time = *time;
        timeout_time.tv_sec += PING_TIMEOUT_S;
        self.waiting_for.insert(OrderedTimeSpec(timeout_time), self.seq);

        self.seq += 1;

        Ok(None)
    }

    fn check_timeouts(&mut self, time: &TimeSpec) -> Result<Option<()>> {
        let remote_host = self.remote_host;

        // Loop until we find a timeout that is still in the past
        while let Some((&ts, &seq)) = self.waiting_for.first_key_value() {
            // ts is &OrderedTimeSpec, so ts.0 is the inner TimeSpec
            if ts.0.tv_sec > time.tv_sec {
                // This entry is in the future, stop removing entries
                break;
            }
            // This one timed out
            println!("From {} icmp_seq={} timeout", remote_host, seq);
            self.waiting_for.pop_first();
        }

        Ok(None)
    }

    fn is_finished(&self) -> Result<Option<()>> {
        if self.packets_to_send != 0
            && self.seq == self.packets_to_send
            && self.waiting_for.is_empty()
        {
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
        if arg == "--help" || arg == "-h" {
            println!("{}", PING_MAN);
            return Ok(());
        } else if arg.starts_with("-i") {
            let value = if arg.len() > 2 {
                // Option value concatenated directly to the flag, e.g., "-i34"
                arg[2..].to_string()
            } else {
                // Option value provided as next argument
                args.next()
                    .ok_or_else(|| anyhow!("No argument to -i option"))?
            };
            interval =
                i64::from_str(&value).map_err(|e| anyhow!("{e}: Invalid argument to -i option"))?;
            if interval <= 0 {
                bail!("Interval can't be less or equal to 0");
            }
        } else if arg.starts_with("-c") {
            let value = if arg.len() > 2 {
                // Option value concatenated directly to the flag, e.g., "-c34"
                arg[2..].to_string()
            } else {
                // Option value provided as next argument
                args.next()
                    .ok_or_else(|| anyhow!("No argument to -c option"))?
            };
            count = usize::from_str(&value)
                .map_err(|e| anyhow!("{e}: Invalid argument to -c option"))?;
        } else {
            if remote_host.is_empty() {
                remote_host = arg.to_owned();
            } else {
                bail!("Too many hosts to ping");
            }
        }
    }

    let remote_host = resolve_host(&remote_host)?;

    let data_size = ECHO_PAYLOAD_SIZE;
    let total_size = data_size + IP_HEADER_SIZE + ICMP_HEADER_SIZE;
    // Print the line similar to standard ping output
    println!(
        "PING {} ({}) {}({}) bytes of data.",
        remote_host, remote_host, data_size, total_size
    );

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

    let event_queue = EventQueue::<EventSource>::new().context("Failed to create event queue")?;

    event_queue.subscribe(echo_fd.raw(), EventSource::Echo, EventFlags::READ)?;
    event_queue.subscribe(time_fd.raw(), EventSource::Time, EventFlags::READ)?;

    let mut ping = Ping::new(remote_host, count, interval, echo_fd, time_fd);

    // Send the first ping immediately
    let current_time = libredox::call::clock_gettime(libredox::flag::CLOCK_MONOTONIC)
        .context("Failed to get the current time")?;
    ping.send_ping(&current_time)?;

    // Schedule the next time event
    let mut buf = [0_u8; mem::size_of::<TimeSpec>()];
    let time = libredox::data::timespec_from_mut_bytes(&mut buf);
    time.tv_sec = current_time.tv_sec + interval;
    time.tv_nsec = current_time.tv_nsec;
    ping.time_file
        .write(&buf)
        .context("Failed to write to time file")?;

    // Start the event loop
    for event_res in event_queue {
        match event_res {
            Ok(event) => {
                let done = match event.user_data {
                    EventSource::Echo => ping.on_echo_event(),
                    EventSource::Time => ping.on_time_event(),
                };

                if let Some(_) = done? {
                    break;
                }
            }
            Err(e) => {
                // Handle Interrupted system call error
                if e.errno() == EINTR {
                    println!("Interrupted! Exiting gracefully.");
                    break;
                }
                eprintln!("Event queue error: {:?}", e);
                break;
            }
        }
    }

    let transmitted = ping.get_transmitted();
    let received = ping.get_recieved();
    println!("--- {} ping statistics ---", remote_host);
    println!(
        "{} packets transmitted, {} packets received, {}% packet loss",
        transmitted,
        received,
        if transmitted > 0 {
            100 * (transmitted - received) / transmitted
        } else {
            0
        }
    );
    Ok(())
}
