use libredox::data::TimeSpec;
use libredox::Fd;

use std::collections::{BTreeMap, HashMap};
use std::mem;
use std::net::IpAddr;
use std::ops::{Deref, DerefMut};
use std::slice;

use crate::stats::PingStatistics;

use anyhow::{bail, Context, Result};
use std::cmp::Ordering;
use std::fmt;

use ECHO_PAYLOAD_SIZE;
const ECHO_PAYLOAD_STRUCT_SIZE: usize = mem::size_of::<EchoPayload>();

use time_diff_ms;
use PING_TIMEOUT_S;

/// A wrapper around libredox::data::TimeSpec that adds trait implementations
/// like PartialEq, Debug, and ordering traits for usage in data structures.
///
/// **Note this wrapper type is necessary because TimeSpec
/// (from libredox crate) does not implement these traits
///
#[derive(Clone, Copy)]
pub struct OrderedTimeSpec(libredox::data::TimeSpec);

impl PartialEq for OrderedTimeSpec {
    /// Checks for equality between two OrderedTimeSpec instances.
    ///
    /// Two OrderedTimeSpec instances are considered equal if both the
    /// tv_sec (seconds) and tv_nsec (nanoseconds) fields are equal.
    fn eq(&self, other: &Self) -> bool {
        self.0.tv_sec == other.0.tv_sec // Compare seconds
            && self.0.tv_nsec == other.0.tv_nsec // Compare nanoseconds
    }
}

impl fmt::Debug for OrderedTimeSpec {
    /// This formats the output as:
    /// OrderedTimeSpec { tv_sec: <seconds>, tv_nsec: <nanoseconds> }.
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
    /// Implements the total ordering for OrderedTimeSpec.
    ///
    /// Ord requires a total ordering, meaning any two instances of OrderedTimeSpec
    /// must be comparable. This implementation orders OrderedTimeSpec based on
    /// its inner TimeSpec fields, comparing tv_sec (seconds) first, and if they
    /// are equal, comparing tv_nsec (nanoseconds).
    ///
    /// - tv_sec: Primary ordering field (whole seconds).
    /// - tv_nsec: Secondary ordering field (sub-second precision).
    fn cmp(&self, other: &Self) -> Ordering {
        self.0
            .tv_sec
            .cmp(&other.0.tv_sec) // Compare seconds first
            // If seconds are equal, compare nanoseconds
            .then_with(|| self.0.tv_nsec.cmp(&other.0.tv_nsec))
    }
}

impl PartialOrd for OrderedTimeSpec {
    /// Provides a partial ordering for OrderedTimeSpec by delegating to Ord.to
    ///
    /// This wraps the result of cmp in Some.
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other)) // Delegate to cmp, as total ordering exists
    }
}

#[repr(C, packed)]
struct IcmpEchoHeader {
    type_: u8,
    code: u8,
    checksum: u16,
    identifier: u16,
    seq: u16,
}

#[repr(C)]
struct EchoPayload {
    header: IcmpEchoHeader,
    timestamp: TimeSpec,
    payload: [u8; ECHO_PAYLOAD_SIZE],
}

impl EchoPayload {
    fn from_bytes(bytes: &[u8]) -> &Self {
        unsafe { &*(bytes.as_ptr() as *const EchoPayload) }
    }
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
            )
        }
    }
}

pub struct Ping {
    pub remote_host: IpAddr,
    pub time_file: Fd,
    pub echo_file: Fd,
    pub seq: u16,
    pub received: usize,
    pub identifier: u16,
    pub(crate) waiting_for: BTreeMap<OrderedTimeSpec, u16>,
    pub packets_to_send: usize,
    pub interval: TimeSpec,
    pub sent_count: usize,
    pub stats: PingStatistics,
    seq_to_timeout: HashMap<u16, OrderedTimeSpec>,
    timeout_to_seq: BTreeMap<OrderedTimeSpec, u16>,
}

impl Ping {
    pub fn new(
        remote_host: IpAddr,
        packets_to_send: usize,
        interval: f64,
        echo_file: Fd,
        time_file: Fd,
        identifier: u16,
    ) -> Ping {
        Ping {
            remote_host,
            echo_file,
            time_file,
            seq: 0,
            received: 0,
            waiting_for: BTreeMap::new(),
            packets_to_send,
            interval: Self::interval_to_timespec(interval),
            identifier,
            sent_count: 0,
            stats: PingStatistics::new(),
            seq_to_timeout: HashMap::new(),
            timeout_to_seq: BTreeMap::new(),
        }
    }

    pub fn interval_to_timespec(interval: f64) -> TimeSpec {
        let tv_sec = interval.floor() as i64;
        let tv_nsec = (interval.fract() * 1_000_000_000.0) as i64;
        TimeSpec { tv_sec, tv_nsec }
    }

    pub fn on_echo_event(&mut self) -> Result<Option<()>> {
        let mut messages = Vec::new();
        let mut processed_packets = 0;
        let mut total_received = 0;

        // Batch process all packets
        loop {
            let mut payload = EchoPayload {
                header: IcmpEchoHeader {
                    type_: 0,
                    code: 0,
                    checksum: 0,
                    identifier: 0,
                    seq: 0,
                },
                timestamp: TimeSpec {
                    tv_sec: 0,
                    tv_nsec: 0,
                },
                payload: [0; ECHO_PAYLOAD_SIZE],
            };

            // Read packet
            let read_size = match self.echo_file.read(&mut payload) {
                Ok(n) => n,
                Err(e) if e.is_wouldblock() => break, // No more data
                Err(e) => return Err(e).context("Failed to read from echo file"),
            };

            let reply = EchoPayload::from_bytes(&payload);
            if reply.header.type_ != 0 || reply.header.code != 0 {
                continue; // Not an Echo Reply
            }
            if reply.header.identifier != self.identifier {
                continue; // Not our ping
            }

            // Handle EOF or incomplete reads
            if read_size == 0 {
                break;
            }
            if read_size < ECHO_PAYLOAD_STRUCT_SIZE {
                bail!(
                    "Malformed packet: Expected {} bytes, got {}",
                    ECHO_PAYLOAD_STRUCT_SIZE,
                    read_size
                );
            }

            processed_packets += 1;
            let current_time = libredox::call::clock_gettime(libredox::flag::CLOCK_MONOTONIC)
                .context("Failed to get current time")?;

            // Match received packet sequence number with sent packets
            let seq = payload.header.seq;
            match self.seq_to_timeout.remove(&seq) {
                Some(timeout_ts) => {
                    // Remove from timeout tracking
                    self.timeout_to_seq.remove(&timeout_ts);

                    // Compute round-trip time
                    let rtt = time_diff_ms(&payload.timestamp, &current_time);
                    self.stats.record_received(rtt);
                    total_received += 1;

                    // Buffer output for batch printing
                    let seq = payload.header.seq;
                    messages.push(format!(
                        "From {} icmp_seq={} time={:.2}ms",
                        self.remote_host, seq, rtt
                    ));
                }
                None => {
                    // Unexpected response (e.g., duplicate or invalid)
                    self.stats.record_error();
                    let seq = payload.header.seq;
                    messages.push(format!(
                        "From {} unexpected icmp_seq={} (duplicate or invalid)",
                        self.remote_host, seq
                    ));
                }
            }
        }

        // Batch print all messages for efficiency
        if !messages.is_empty() {
            println!("{}", messages.join("\n"));
        }

        // Ensure `total_sent` is properly counted in `send_ping()`
        self.received += total_received;

        self.is_finished()
    }

    pub fn on_time_event(&mut self) -> Result<Option<()>> {
        let mut buf = [0_u8; mem::size_of::<TimeSpec>()];
        self.time_file.read(&mut buf)?; // discard

        // Get the real monotonic time for sending a new ping & timeouts
        let now = libredox::call::clock_gettime(libredox::flag::CLOCK_MONOTONIC)
            .context("Failed to get the current time")?;

        // First check timeouts for existing pings
        self.check_timeouts(&now)?;

        // Only send new ping and schedule next event if we haven't reached the limit
        if self.packets_to_send == 0 || (self.seq as usize) < self.packets_to_send {
            // Send the ping
            self.send_ping(&now)?;

            // Schedule the *next* alarm event at now + self.interval
            let mut alarm_time = now;
            alarm_time.tv_sec += self.interval.tv_sec;
            alarm_time.tv_nsec += self.interval.tv_nsec;

            // Handle nanosecond overflow
            if alarm_time.tv_nsec >= 1_000_000_000 {
                alarm_time.tv_sec += 1;
                alarm_time.tv_nsec -= 1_000_000_000;
            }

            // Serialize alarm_time into a byte buffer and write it
            let mut alarm_buf = [0_u8; mem::size_of::<TimeSpec>()];
            {
                let alarm_spec = libredox::data::timespec_from_mut_bytes(&mut alarm_buf);
                *alarm_spec = alarm_time;
            }
            self.time_file
                .write(&alarm_buf)
                .context("Failed to write the next alarm time")?;
        }

        self.is_finished()
    }

    fn icmp_checksum(data: &[u8]) -> u16 {
        let mut sum = 0u32;
        let mut i = 0;
        while i + 1 < data.len() {
            let word = ((data[i] as u16) << 8) | (data[i + 1] as u16);
            sum = sum.wrapping_add(word as u32);
            i += 2;
        }
        // If there is a leftover byte, pad with zero.
        if i < data.len() {
            sum = sum.wrapping_add(((data[i] as u16) as u32) << 8);
        }
        // Add carries from top 16 bits to lower 16 bits.
        while (sum >> 16) != 0 {
            sum = (sum & 0xFFFF) + (sum >> 16);
        }
        !(sum as u16)
    }

    pub fn send_ping(&mut self, time: &TimeSpec) -> Result<Option<()>> {
        // Check packet count limit
        if self.packets_to_send != 0 && (self.seq as usize) >= self.packets_to_send {
            return Ok(Some(()));
        }

        // Construct ping payload
        let mut payload = EchoPayload {
            header: IcmpEchoHeader {
                type_: 8, // ICMP Echo Request
                code: 0,
                checksum: 0,
                identifier: self.identifier,
                seq: self.seq,
            },
            timestamp: *time,
            payload: [0; ECHO_PAYLOAD_SIZE],
        };

        // Compute checksum:
        let payload_bytes = unsafe {
            std::slice::from_raw_parts(
                &payload as *const EchoPayload as *const u8,
                std::mem::size_of::<EchoPayload>(),
            )
        };

        payload.header.checksum = Self::icmp_checksum(payload_bytes);

        // Write payload to echo file
        self.echo_file
            .write(&payload)
            .context("Failed to write ping payload to echo file")?;

        self.sent_count += 1;

        // Calculate timeout timestamp
        let mut timeout_time = *time;
        timeout_time.tv_sec += PING_TIMEOUT_S;
        let timeout_ts = OrderedTimeSpec(timeout_time);

        // Update tracking structures
        self.seq_to_timeout.insert(self.seq, timeout_ts);
        self.timeout_to_seq.insert(timeout_ts, self.seq);

        // Increment sequence number
        self.seq = self.seq.wrapping_add(1);

        // Update statistics
        self.stats.record_sent();

        Ok(None)
    }

    pub fn print_final_statistics(&self) {
        self.stats.print_summary(self.remote_host);
    }

    fn is_finished(&self) -> Result<Option<()>> {
        let all_packets_sent =
            self.packets_to_send == 0 || (self.seq as u32) >= (self.packets_to_send as u32);

        let no_pending_requests = self.seq_to_timeout.is_empty();

        if all_packets_sent && no_pending_requests {
            Ok(Some(()))
        } else {
            Ok(None)
        }
    }

    fn check_timeouts(&mut self, time: &TimeSpec) -> Result<Option<()>> {
        let now = OrderedTimeSpec(*time);

        while let Some((ts, seq)) = self.timeout_to_seq.pop_first() {
            if ts > now {
                self.timeout_to_seq.insert(ts, seq);
                break;
            }

            self.seq_to_timeout.remove(&seq);
            self.stats.record_error(); // Directly record timeout as error
            println!("From {} icmp_seq={} timeout", self.remote_host, seq);
        }

        Ok(None)
    }
}
