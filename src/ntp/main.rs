#![deny(warnings)]

extern crate ntpclient;

use ntpclient::retrieve_ntp_timestamp;
use std::env;

fn format_time(mut ts: i64) -> String {
    let s = ts%86400;
    ts /= 86400;
    let h = s/3600;
    let m = s/60%60;
    let s = s%60;
    let x = (ts*4+102032)/146097+15;
    let b = ts+2442113+x-(x/4);
    let mut c = (b*20-2442)/7305;
    let d = b-365*c-c/4;
    let mut e = d*1000/30601;
    let f = d-e*30-e*601/1000;
    if e < 14 {
        c -= 4716;
        e -= 1;
    } else {
        c -= 4715;
        e -= 13;
    }
    format!("{:>04}-{:>02}-{:>02} {:>02}:{:>02}:{:>02}", c, e, f, h, m, s)
}

fn main() {
    let server = env::args().nth(1).unwrap_or("pool.ntp.org".to_string());
    let ntp_time = retrieve_ntp_timestamp(&server).unwrap();
    println!("{}: {}", server, format_time(ntp_time.sec));
}
