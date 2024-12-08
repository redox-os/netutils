mod ping;
mod stats;
use ping::Ping;

use std::net::IpAddr;
extern crate anyhow;
extern crate event;
extern crate libredox;

use std::env::args;
use std::mem;

use std::net::ToSocketAddrs;

use std::str::FromStr;

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
const MICROSECONDS_PER_MILLISECOND: i64 = 1_000;
const _NANOSECONDS_PER_SECOND: i64 = 1_000_000_000;

fn resolve_host(host: &str) -> Result<IpAddr> {
    match (host, 0).to_socket_addrs()?.next() {
        Some(addr) => Ok(addr.ip()),
        None => Err(anyhow!("Failed to resolve remote host's IP address")),
    }
}

/// Computes the difference between `from` and `to` in milliseconds,
/// taking into account both the seconds (`tv_sec`) and nanoseconds (`tv_nsec`) fields
/// of the `TimeSpec` structure.
///
/// # Notes
/// - The result is signed, meaning it can be negative if `from` is after `to`.
/// - This assumes that `tv_nsec` values are less than 1 second (valid for `TimeSpec`).
///
/// # Example
/// let from = TimeSpec { tv_sec: 10, tv_nsec: 500_000_000 }; // 10.5 seconds
/// let to = TimeSpec { tv_sec: 12, tv_nsec: 0 };            // 12.0 seconds
/// assert_eq!(time_diff_ms(&from, &to), 1500.0);            // 1.5 seconds = 1500 ms
///
fn time_diff_ms(from: &TimeSpec, to: &TimeSpec) -> f32 {
    let seconds_diff = (to.tv_sec - from.tv_sec) * 1_000_000;
    let nanoseconds_diff = ((to.tv_nsec - from.tv_nsec) as i64) / MICROSECONDS_PER_MILLISECOND;

    (seconds_diff + nanoseconds_diff) as f32 / 1_000.0
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

    ping.print_final_statistics();

    Ok(())
}
