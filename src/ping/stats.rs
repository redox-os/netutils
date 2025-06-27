///stats.rs
use std::net::IpAddr;

pub struct PingStatistics {
    pub total_sent: u32,
    pub total_received: u32,
    pub total_errors: u32,
    pub min_rtt: Option<f32>,
    pub max_rtt: Option<f32>,
    pub avg_rtt: f32,
    pub rtts: Vec<f32>,
}

impl PingStatistics {
    pub fn new() -> Self {
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

    pub fn record_sent(&mut self) {
        self.total_sent += 1;
    }

    pub fn record_received(&mut self, rtt: f32) {
        self.total_received += 1;

        // Update RTT tracking
        self.rtts.push(rtt);

        // Update min/max RTT
        self.min_rtt = Some(self.min_rtt.map_or(rtt, |current| current.min(rtt)));
        self.max_rtt = Some(self.max_rtt.map_or(rtt, |current| current.max(rtt)));

        // Recalculate average
        self.avg_rtt = self.rtts.iter().sum::<f32>() / self.rtts.len() as f32;
    }

    pub fn record_error(&mut self) {
        self.total_errors += 1;
    }

    fn packet_loss_percentage(&self) -> f32 {
        if self.total_sent == 0 {
            0.0
        } else {
            ((self.total_sent - self.total_received) as f32 / self.total_sent as f32) * 100.0
        }
    }

    pub fn print_summary(&self, remote_host: IpAddr) {
        println!("--- {remote_host} ping statistics ---");
        println!(
            "{} packets transmitted, {} packets received, {:.2}% packet loss",
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
