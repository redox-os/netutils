// main.rs
// Entry point for the ifconfig utility on Redox OS.

extern crate regex;
use std::env;

mod interface;
use interface::*; // Module to handle interface-related logic

static MAN_PAGE: &str = /* @MANSTART{ifconfig} */
    r#"
NAME
    ifconfig - Configure or display network interfaces

SYNOPSIS
    ifconfig [-h | --help] [-a] interface

DESCRIPTION
    Displays and/or configures network interfaces.

OPTIONS
    -h
    --help
        Print this manual page.
    -a
        Display information about all available interfaces in the system.
        interface
        This parameter is a string of the form "name unit", for example "eth0".

HISTORY
    The ifconfig utility appears in redox-os xxx.

AUTHOR
    Written by G. Gielly.
"#; /* @MANEND */

fn main() {
    // Collect command-line arguments, skipping the program name
    let mut args = env::args().skip(1);
    let mut show_all = false;
    let mut interface_name = None;

    // Parse command-line arguments
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-h" | "--help" => {
                // Display the manual page
                println!("{}", MAN_PAGE);
                return;
            }
            "-a" => {
                // Set flag to show all interfaces
                show_all = true;
            }
            _ => {
                // Capture the interface name if provided
                if interface_name.is_none() {
                    interface_name = Some(arg);
                } else {
                    // Handle invalid arguments
                    eprintln!("Invalid argument: {}", arg);
                    return;
                }
            }
        }
    }

    // Determine behavior based on parsed arguments
    if show_all {
        if let Some(name) = interface_name {
            // Display details for the specific interface if it exists
            match NetworkInterface::new(&name) {
                Ok(interface) => println!("{}", interface),
                Err(_) => eprintln!("Error: Interface '{}' not found.", name),
            }
        } else {
            // Display all interfaces if no specific name is provided
            match list_all_interfaces() {
                Ok(interfaces) => {
                    if interfaces.is_empty() {
                        println!("No interfaces found.");
                    } else {
                        for interface in interfaces {
                            println!("{}", interface);
                            println!(); // Add an empty line between interfaces
                        }
                    }
                }
                Err(e) => eprintln!("Error listing interfaces: {}", e),
            }
        }
    } else if let Some(name) = interface_name {
        // Show details for a specific interface without `-a`
        match NetworkInterface::new(&name) {
            Ok(interface) => println!("{}", interface),
            Err(_) => eprintln!("Error: Interface '{}' not found.", name),
        }
    } else {
        // Default behavior: Show all interfaces if no arguments are provided
        match list_all_interfaces() {
            Ok(interfaces) => {
                if interfaces.is_empty() {
                    println!("No interfaces found.");
                } else {
                    for interface in interfaces {
                        println!("{}", interface);
                        println!(); // Add an empty line between interfaces
                    }
                }
            }
            Err(e) => eprintln!("Error listing interfaces: {}", e),
        }
    }
}
