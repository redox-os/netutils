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
const NANOSECONDS_PER_SECOND: i64 = 1_000_000_000;
const MICROSECONDS_PER_MILLISECOND: i64 = 1_000;

/// A wrapper around `libredox::data::TimeSpec` that adds trait implementations
/// like `PartialEq`, `Debug`, and ordering traits for usage in data structures.
///
/// **Note this wrapper type is necessary because `TimeSpec`
/// (from `libredox` crate) does not implement these traits
///
#[derive(Clone, Copy)] // Allows cheap copying of `OrderedTimeSpec` values
struct OrderedTimeSpec(libredox::data::TimeSpec);

impl PartialEq for OrderedTimeSpec {
    /// Checks for equality between two `OrderedTimeSpec` instances.
    ///
    /// Two `OrderedTimeSpec` instances are considered equal if both the
    /// `tv_sec` (seconds) and `tv_nsec` (nanoseconds) fields are equal.
    ///
    /// # Example
    ///
    /// let a = OrderedTimeSpec(TimeSpec { tv_sec: 10, tv_nsec: 100 });
    /// let b = OrderedTimeSpec(TimeSpec { tv_sec: 10, tv_nsec: 100 });
    /// assert!(a == b);
    ///
    /// let c = OrderedTimeSpec(TimeSpec { tv_sec: 10, tv_nsec: 200 });
    /// assert!(a != c);
    ////
    fn eq(&self, other: &Self) -> bool {
        self.0.tv_sec == other.0.tv_sec // Compare seconds
            && self.0.tv_nsec == other.0.tv_nsec // Compare nanoseconds
    }
}

impl fmt::Debug for OrderedTimeSpec {
    /// This formats the output as:
    /// `OrderedTimeSpec { tv_sec: <seconds>, tv_nsec: <nanoseconds> }`.
    ///
    /// # Example Output
    /// let a = OrderedTimeSpec(TimeSpec { tv_sec: 10, tv_nsec: 200 });
    /// println!("{:?}", a); // Output: OrderedTimeSpec { tv_sec: 10, tv_nsec: 200 }
    ///
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "OrderedTimeSpec {{ tv_sec: {}, tv_nsec: {} }}",
            self.0.tv_sec,  // Include seconds in output
            self.0.tv_nsec  // Include nanoseconds in output
        )
    }
}

impl Eq for OrderedTimeSpec {}

impl Ord for OrderedTimeSpec {
    /// Implements the total ordering for `OrderedTimeSpec`.
    ///
    /// `Ord` requires a total ordering, meaning any two instances of `OrderedTimeSpec`
    /// must be comparable. This implementation orders `OrderedTimeSpec` based on
    /// its inner `TimeSpec` fields, comparing `tv_sec` (seconds) first, and if they
    /// are equal, comparing `tv_nsec` (nanoseconds).
    ///
    /// - `tv_sec`: Primary ordering field (whole seconds).
    /// - `tv_nsec`: Secondary ordering field (sub-second precision).
    fn cmp(&self, other: &Self) -> Ordering {
        self.0
            .tv_sec
            .cmp(&other.0.tv_sec) // Compare seconds first
            // If seconds are equal, compare nanoseconds
            .then_with(|| self.0.tv_nsec.cmp(&other.0.tv_nsec))
    }
}

impl PartialOrd for OrderedTimeSpec {
    /// Provides a partial ordering for `OrderedTimeSpec` by delegating to `Ord`.
    ///
    /// `PartialOrd` is required for types that can be compared, but not all
    /// comparisons must yield a result. For `OrderedTimeSpec`, a total ordering
    /// exists (via `Ord`), so `partial_cmp` always returns `Some(Ordering)`.
    ///
    /// This wraps the result of `cmp` in `Some`.
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other)) // Delegate to `cmp`, as total ordering exists
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
    seq: u16, // Changed from usize to u16, (max 65 535, ICMP spec)
    received: usize,
    //We replace the Vec with BTreeMap
    waiting_for: BTreeMap<OrderedTimeSpec, u16>, // Changed from usize to u16
    packets_to_send: usize,
    interval: i64,
    stats: PingStatistics,
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
            seq: 0, // still 0 in u16
            received: 0,
            // Initialize as a BTreeMap
            waiting_for: BTreeMap::new(),
            packets_to_send,
            interval,
            stats: PingStatistics::new(),
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

        if self.received > 0 {
            let time = libredox::call::clock_gettime(libredox::flag::CLOCK_MONOTONIC)
                .context("Failed to get the current time")?;
            let rtt = time_diff_ms(&payload.timestamp, &time);
            self.stats.record_received(rtt);
        } else {
            self.stats.record_error();
        }

        if readed == 0 {
            return Ok(None);
        }

        if readed < mem::size_of::<EchoPayload>() {
            bail!("Not enough data in the echo file");
        }

        let time = libredox::call::clock_gettime(libredox::flag::CLOCK_MONOTONIC)
            .context("Failed to get the current time")?;

        let remote_host = self.remote_host;

        let mut received = 0;
        self.waiting_for.retain(|_ts, &mut seq| {
            if seq as u16 == payload.seq {
                received += 1;
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
        self.received += received;
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
        if self.packets_to_send != 0 && usize::from(self.seq) >= self.packets_to_send {
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
        self.waiting_for
            .insert(OrderedTimeSpec(timeout_time), self.seq);

        self.seq += 1;

        self.stats.record_sent();

        Ok(None)
    }

    fn print_final_statistics(&self) {
        self.stats.print_summary(self.remote_host);
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
            && usize::from(self.seq) == self.packets_to_send
            && self.waiting_for.is_empty()
        {
            Ok(Some(()))
        } else {
            Ok(None)
        }
    }

    fn get_transmitted(&self) -> u16 {
        self.seq
    }

    fn get_received(&self) -> usize {
        self.received
    }
}

struct PingStatistics {
    total_sent: u32,
    total_received: u32,
    total_errors: u32,
    min_rtt: Option<f32>,
    max_rtt: Option<f32>,
    avg_rtt: f32,
    rtts: Vec<f32>,
}

impl PingStatistics {
    fn new() -> Self {
        PingStatistics {
            total_sent: 0,
            total_received: 0,
            total_errors: 0,
            min_rtt: None,
            max_rtt: None,
            avg_rtt: 0.0,
            rtts: Vec::new(),
        }
    }

    fn record_sent(&mut self) {
        self.total_sent += 1;
    }

    fn record_received(&mut self, rtt: f32) {
        self.total_received += 1;

        // Update RTT tracking
        self.rtts.push(rtt);

        // Update min/max RTT
        self.min_rtt = Some(self.min_rtt.map_or(rtt, |current| current.min(rtt)));
        self.max_rtt = Some(self.max_rtt.map_or(rtt, |current| current.max(rtt)));

        // Recalculate average
        self.avg_rtt = self.rtts.iter().sum::<f32>() / self.rtts.len() as f32;
    }

    fn record_error(&mut self) {
        self.total_errors += 1;
    }

    fn packet_loss_percentage(&self) -> f32 {
        if self.total_sent == 0 {
            0.0
        } else {
            ((self.total_sent - self.total_received) as f32 / self.total_sent as f32) * 100.0
        }
    }

    fn print_summary(&self, remote_host: IpAddr) {
        println!("--- {} ping statistics ---", remote_host);
        println!(
            "{} packets transmitted, {} received, {:.2}% packet loss",
            self.total_sent,
            self.total_received,
            self.packet_loss_percentage()
        );

        if !self.rtts.is_empty() {
            println!(
                "rtt min/avg/max = {:.3}/{:.3}/{:.3} ms",
                self.min_rtt.unwrap(),
                self.avg_rtt,
                self.max_rtt.unwrap()
            );
        }
    }
}

fn resolve_host(host: &str) -> Result<IpAddr> {
    match (host, 0).to_socket_addrs()?.next() {
        Some(addr) => Ok(addr.ip()),
        None => Err(anyhow!("Failed to resolve remote host's IP address")),
    }
}

/// Calculates the time difference in milliseconds between two `TimeSpec` instances.
///
/// Computes the difference between `from` and `to` in milliseconds,
/// taking into account both the seconds (`tv_sec`) and nanoseconds (`tv_nsec`) fields
/// of the `TimeSpec` structure.
///
/// # Parameters
/// - `from`: The earlier time (`TimeSpec`) to subtract from.
/// - `to`: The later time (`TimeSpec`) to subtract against.
///
/// # Returns
/// - The time difference in milliseconds => `f32`.
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

    //let _transmitted = ping.get_transmitted();
    //let _received = ping.get_received();

    ping.print_final_statistics();

    /*
    println!("--- {} ping statistics ---", remote_host);
    println!(
        "{} packets transmitted, {} packets received, {}% packet loss",
        transmitted,
        received,
        if transmitted > 0 {
            100 * ((transmitted as usize) - received) / (transmitted as usize)
        } else {
            0
        }
    );*/

    Ok(())
}
