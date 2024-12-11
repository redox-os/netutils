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

//use DEFAULT_TTL;
use ECHO_PAYLOAD_SIZE;

use time_diff_ms;
use PING_TIMEOUT_S;

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
    pub seq: u16, // Changed from usize to u16, (max 65 535, ICMP spec)
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
            seq: 0, // still 0 in u16
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

    pub fn send_ping(&mut self, time: &TimeSpec) -> Result<Option<()>> {
        if self.packets_to_send != 0 && usize::from(self.seq) >= self.packets_to_send {
            return Ok(None);
        }

        let payload = EchoPayload {
            seq: self.seq as u16,
            timestamp: *time,
            //ttl: self.ttl,
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

}
