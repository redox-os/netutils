mod ping;
mod stats;
use ping::Ping;

use anyhow::{anyhow, bail, Context, Result};
use clap::{Arg, ArgAction, Command};
use event::{user_data, EventFlags, EventQueue};
use std::mem;
use std::net::IpAddr;
use std::net::ToSocketAddrs;

use libredox::data::TimeSpec;
use libredox::errno::EINTR;
use libredox::{flag, Fd};

/*
static PING_MAN: &'static str = /* @MANSTART{ping} */
    r#"
NAME
    ping - send ICMP ECHO_REQUEST to network hosts

SYNOPSIS
    ping [-h | --help] [-c count] [-i interval] [-t ttl] destination

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

"#; /* @MANEND */ */

const PING_TIMEOUT_S: i64 = 5;
const ECHO_PAYLOAD_SIZE: usize = 40;
const IP_HEADER_SIZE: usize = 20;
const ICMP_HEADER_SIZE: usize = 8;

const MICROSECONDS_PER_MILLISECOND: i64 = 1_000;
//const NANOSECONDS_PER_SECOND: i64 = 1_000_000_000;

// TODO : add the ttl feature
//const DEFAULT_TTL: u8 = 64;
//const MAX_TTL: u8 = 255;
//const PING_PACKETS_TO_SEND: usize = 4;
//const PING_INTERVAL_S: i64 = 1;

fn resolve_host(host: &str) -> Result<IpAddr> {
    match (host, 0).to_socket_addrs()?.next() {
        Some(addr) => Ok(addr.ip()),
        None => {
            println!("Failed to resolve host: {host}");
            Err(anyhow!("Failed to resolve remote host's IP address"))
        }
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

fn parse_args() -> Result<(String, usize, i64)> {
    let matches = Command::new("ping")
        .about("send ICMP ECHO_REQUEST to network hosts")
        //.after_help(PING_MAN)
        .arg(
            Arg::new("destination")
                .help("The host to ping (an IPv4 address or hostname)")
                .required(true)
                .action(ArgAction::Set),
        )
        .arg(
            Arg::new("count")
                .short('c')
                .long("count")
                .value_name("COUNT")
                .help("Number of packets to send (0 means send until interrupted).")
                .default_value("4")
                .num_args(1)
                .action(ArgAction::Set),
        )
        .arg(
            Arg::new("interval")
                .short('i')
                .long("interval")
                .value_name("INTERVAL")
                .help("Wait interval seconds before sending next packet.")
                .default_value("1")
                .num_args(1)
                .action(ArgAction::Set),
        )
        // TODO : TTL
        // The TTL feature has been removed because icmp/ttl is not ready.
        // If needed in the future, uncomment the following code and add the u8 in the function
        //  .arg(
        //    Arg::new("ttl")
        //        .short('t')
        //        .long("ttl")
        //        .value_name("TTL")
        //        .help(&format!(
        //            "Set the IP Time To Live for outgoing packets (default: {}, range: 1-{})",
        //            DEFAULT_TTL, MAX_TTL
        //        ))
        //        .default_value(DEFAULT_TTL)
        //        .num_args(1)
        //        .action(ArgAction::Set),
        // )
        //
        .get_matches();

    let remote_host = matches
        .get_one::<String>("destination")
        .expect("destination required by clap")
        .to_string();

    let count_str = matches
        .get_one::<String>("count")
        .expect("count should have a default and thus always be present");
    let count: usize = count_str
        .parse()
        .map_err(|e| anyhow!("Invalid packet count for -c: {} ({})", count_str, e))?;

    let interval_str = matches
        .get_one::<String>("interval")
        .expect("interval should have a default");
    let interval: i64 = interval_str
        .parse()
        .map_err(|e| anyhow!("Invalid interval value for -i: {} ({})", interval_str, e))?;
    if interval <= 0 {
        bail!("Interval must be a positive number");
    }

    // TODO : TTL
    // let ttl_str = matches
    //    .get_one::<String>("ttl")
    //    .expect("ttl should have a default");
    // let ttl: u8 = ttl_str
    //    .parse()
    //    .map_err(|e| anyhow!("Invalid TTL value for -t: {} ({})", ttl_str, e))?;
    // if !(1..=MAX_TTL).contains(&ttl) {
    //    bail!("TTL must be between 1 and {}", MAX_TTL);

    Ok((remote_host, count, interval))
}

fn main() -> Result<()> {
    // Parsing the command line
    let (remote_host, count, interval) = parse_args()?;

    user_data! {
        enum EventSource {
            Echo,
            Time,
        }
    }

    let remote_host = resolve_host(&remote_host)?;

    let data_size = ECHO_PAYLOAD_SIZE;
    let total_size = data_size + IP_HEADER_SIZE + ICMP_HEADER_SIZE;
    // Print the line similar to standard ping output
    println!("PING {remote_host} ({remote_host}) {data_size}({total_size}) bytes of data.");

    // Create the path to the ICMP echo file for the remote host
    let icmp_path = format!("/scheme/icmp/echo/{remote_host}");

    // Open the ICMP echo file in read-write, non-blocking mode
    let echo_fd = Fd::open(&icmp_path, flag::O_RDWR | flag::O_NONBLOCK, 0)
        .map_err(|_| anyhow!("Can't open path {}", icmp_path))?;

    // Create the path to the monotonic clock file
    let time_path = format!("/scheme/time/{}", flag::CLOCK_MONOTONIC);

    // Open the monotonic clock file in read-write mode
    let time_fd = Fd::open(&time_path, flag::O_RDWR, 0)
        .map_err(|_| anyhow!("Can't open path {}", time_path))?;

    // Create a new event queue
    let event_queue = EventQueue::<EventSource>::new().context("Failed to create event queue")?;

    // Subscribe the event queue to read events from the ICMP echo file
    event_queue.subscribe(echo_fd.raw(), EventSource::Echo, EventFlags::READ)?;

    // Subscribe the event queue to read events from the monotonic clock file
    event_queue.subscribe(time_fd.raw(), EventSource::Time, EventFlags::READ)?;

    // Create a new Ping instance with the specified parameters
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

                if done?.is_some() {
                    break;
                }
            }
            Err(e) => {
                // Handle Interrupted system call error
                if e.errno() == EINTR {
                    println!("Interrupted! Exiting gracefully.");
                    break;
                }
                eprintln!("Event queue error: {e:?}");
                break;
            }
        }
    }

    ping.print_final_statistics();

    Ok(())
}
