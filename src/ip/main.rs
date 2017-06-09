extern crate netutils;

use std::fs::File;
use std::env;
use std::str;
use std::io::{Read, Write};

fn open_device() -> Result<File, String> {
    Ok(File::open("ethernet:device").map_err(|e| format!("Failed to open ethernet scheme: {}", e))?)
}

fn get_ips() -> Result<Vec<String>, String> {
    let mut device = open_device()?;
    let mut buf: [u8; 200] = [0; 200];
    device.read(&mut buf).map_err(|e| format!("Failed to read IPs from device: {}", e))?;
    Ok(str::from_utf8(&buf)
        .map_err(|e| format!("Failed to read IPs from device: {}", e))?
        .split(',')
        .skip(1)
        .map(|s| s.to_string())
        .collect())
}

fn set_ip(ip: &str) -> Result<usize, String> {
    let mut device = open_device()?;
    device.write(format!("set_ipv4={}", ip).as_bytes()).map_err(|e| format!("Failed to set ip address {}: {}", ip, e))
}

fn del_ip(ip: &str) -> Result<usize, String> {
    let mut device = open_device()?;
    device.write(format!("del_ipv4={}", ip).as_bytes()).map_err(|e| format!("Failed to delete ip address {}: {}", ip, e))
}

fn main() {
    let mut args = env::args().skip(1);
    match args.next().as_ref().map(String::as_ref) {
        None | Some("show") => {
            match get_ips() {
                Ok(ips) => {
                    if ips.len() == 0 {
                        println!("No IP configured on the device");
                    } else {
                        println!("Current IPs:");
                        for ip in ips.iter() {
                            println!("{}", ip);
                        }
                    }
                },
                Err(e) => {
                    println!("Failed to get IPs: {}", e);
                }
            }
        },
        Some("add") => {
            if let Some(ip) = args.next().as_ref() {
                match set_ip(ip) {
                    Ok(_) => {
                        println!("Ip address set");
                    },
                    Err(e) => {
                        println!("{}", e);
                    }
                }
            } else {
                println!("Missing argument to \"add\"");
            }
        },
        Some("del") => {
            if let Some(ip) = args.next().as_ref() {
                match del_ip(ip) {
                    Ok(_) => {
                        println!("Ip address deleted");
                    },
                    Err(e) => {
                        println!("{}", e);
                    }
                }
            } else {
                println!("Missing argument to \"del\"");
            }
        },
        Some(arg) => {
            println!("Invalid argument \"{}\"", arg);
        },
    }
}
