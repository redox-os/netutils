/// ping.rs
use libredox::data::TimeSpec;
use libredox::Fd;

use std::collections::BTreeMap;
use std::mem;
use std::net::IpAddr;
use std::ops::{Deref, DerefMut};
use std::slice;

use crate::stats::PingStatistics;

use anyhow::{bail, Context, Result};
use std::cmp::Ordering;
use std::fmt;

//use crate::DEFAULT_TTL;  // TODO : TTL
use crate::ECHO_PAYLOAD_SIZE;

use crate::time_diff_ms;
use crate::PING_TIMEOUT_S;

/// A wrapper around `libredox::data::TimeSpec` that adds trait implementations
/// like `PartialEq`, `Debug`, and ordering traits for usage in data structures.
///
/// **Note this wrapper type is necessary because `TimeSpec`
/// (from `libredox` crate) does not implement these traits
///
#[derive(Clone, Copy)] // Allows cheap copying of `OrderedTimeSpec` values
pub struct OrderedTimeSpec(libredox::data::TimeSpec);

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
    /// This wraps the result of `cmp` in `Some`.
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other)) // Delegate to `cmp`, as total ordering exists
    }
}

#[repr(C)]
struct EchoPayload {
    seq: u16,
    timestamp: TimeSpec,
    //ttl: u8,
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

pub struct Ping {
    pub remote_host: IpAddr,
    pub time_file: Fd,
    pub echo_file: Fd,
    pub seq: u16, // Changed from usize to u16 (max 65 535, ICMP spec)
    pub received: usize,
    //We replace the Vec with BTreeMap and reduce visibility here
    pub(crate) waiting_for: BTreeMap<OrderedTimeSpec, u16>,
    pub packets_to_send: usize,
    pub interval: i64,
    pub stats: PingStatistics,
    //pub ttl: u8,
}

impl Ping {
    pub fn new(
        remote_host: IpAddr,
        packets_to_send: usize,
        interval: i64,
        echo_file: Fd,
        time_file: Fd,
        //ttl: Option<u8>,
    ) -> Ping {
        Ping {
            remote_host,
            echo_file,
            time_file,
            seq: 0,
            received: 0,
            // Initialize as a BTreeMap
            waiting_for: BTreeMap::new(),
            packets_to_send,
            interval,
            stats: PingStatistics::new(),
            //ttl: ttl.unwrap_or(DEFAULT_TTL),
        }
    }

    pub fn on_echo_event(&mut self) -> Result<Option<()>> {
        // Read an ICMP echo reply into a fresh payload buffer.
        let mut payload = EchoPayload {
            seq: 0,
            timestamp: TimeSpec {
                tv_sec: 0,
                tv_nsec: 0,
            },
            //ttl: 0,
            payload: [0; ECHO_PAYLOAD_SIZE],
        };

        let readed = match self.echo_file.read(&mut payload) {
            Ok(0) => {
                // No data – treat as an error condition.
                self.stats.record_error();
                return Ok(None);
            }
            Ok(cnt) => cnt,
            Err(e) if e.is_wouldblock() => return Ok(None),
            Err(e) => return Err(e).context("Failed to read from echo file"),
        };

        if readed < mem::size_of::<EchoPayload>() {
            bail!("Not enough data in the echo file");
        }

        // Compute round‑trip time.
        let now = libredox::call::clock_gettime(libredox::flag::CLOCK_MONOTONIC)
            .context("Failed to get the current time")?;
        let rtt = time_diff_ms(&payload.timestamp, &now);

        // Look for a pending request that matches the received sequence number.
        if let Some((&ts, _)) = self.waiting_for.iter().find(|(_, &seq)| seq == payload.seq) {
            // Matching entry found – remove it, record success and print the result.
            self.waiting_for.remove(&ts);
            println!(
                "From {} icmp_seq={} time={}ms",
                self.remote_host, payload.seq, rtt
            );
            self.stats.record_received(rtt);
            self.received += 1;
        } else {
            // No matching request – count as an error.
            self.stats.record_error();
        }

        // Determine whether the ping session is complete.
        self.is_finished()
    }

    pub fn on_time_event(&mut self) -> Result<Option<()>> {
        //  Read from the 'time:' file just to consume the alarm event,
        //  but do *not* treat it as the current time for RTT.
        let mut buf = [0_u8; mem::size_of::<TimeSpec>()];
        self.time_file.read(&mut buf)?; // discard

        // Get the real monotonic time for sending a new ping & timeouts
        let now = libredox::call::clock_gettime(libredox::flag::CLOCK_MONOTONIC)
            .context("Failed to get the current time")?;
        self.send_ping(&now)?;
        self.check_timeouts(&now)?;

        // Schedule the *next* alarm event at now + self.interval
        let mut alarm_time = now;
        alarm_time.tv_sec += self.interval;

        // Serialize alarm_time into a byte buffer and write it
        let mut alarm_buf = [0_u8; mem::size_of::<TimeSpec>()];
        {
            let alarm_spec = libredox::data::timespec_from_mut_bytes(&mut alarm_buf);
            *alarm_spec = alarm_time;
        }
        self.time_file
            .write(&alarm_buf)
            .context("Failed to write the next alarm time")?;

        // If we've sent all packets and have no outstanding replies, finish
        self.is_finished()
    }

    pub fn send_ping(&mut self, time: &TimeSpec) -> Result<Option<()>> {
        if self.packets_to_send != 0 && usize::from(self.seq) >= self.packets_to_send {
            return Ok(None);
        }

        let payload = EchoPayload {
            seq: self.seq,
            timestamp: *time,
            // ttl: self.ttl,
            payload: [1; ECHO_PAYLOAD_SIZE],
        };

        /* TODO : Set TTL for the echo file
        The icmp:echo scheme might not support setting the TTL this way
        resulting in EINVAL (Invalid Argument).
        let ttl_path = format!("icmp:echo/{}/ttl", self.remote_host);
        let ttl_fd = Fd::open(&ttl_path, flag::O_WRONLY, 0).context("Failed to open TTL file")?;
        ttl_fd.write(&[self.ttl])?;
        */

        let _ = self.echo_file.write(&payload)?;

        let mut timeout_time = *time;

        timeout_time.tv_sec += PING_TIMEOUT_S;
        self.waiting_for
            .insert(OrderedTimeSpec(timeout_time), self.seq);

        self.seq += 1;

        self.stats.record_sent();

        Ok(None)
    }

    pub fn print_final_statistics(&self) {
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
            println!("From {remote_host} icmp_seq={seq} timeout");
            self.waiting_for.pop_first();
        }

        Ok(None)
    }

    fn is_finished(&self) -> Result<Option<()>> {
        if self.packets_to_send > 0
            && usize::from(self.seq) == self.packets_to_send
            && self.waiting_for.is_empty()
        {
            Ok(Some(()))
        } else {
            Ok(None)
        }
    }
}
