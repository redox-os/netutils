// main.rs

/*
Entry point for the ifconfig utility on Redox OS.

This program implements a basic `ifconfig` utility for Redox OS. It allows users to:

* **Display information** about network interfaces on the system.
* **List all interfaces** with their IP addresses, netmasks, and (placeholder) MAC addresses.
* **Display details** for a specific interface provided as an argument.

The program supports the following options:

* `-h` or `--help`: Prints the help message and exits.
* `-a`: Shows information about all available interfaces.

**Limitations:**

* Currently, the program cannot configure network interfaces (work in progress).
* The displayed MAC address is a placeholder.

*/

use std::env;

mod interface;
use interface::*; // Module to handle interface-related logic

static MAN_PAGE: &str = /* @MANSTART{ifconfig} */
    r#"
NAME
    ifconfig - Configure or display network interfaces

SYNOPSIS
    ifconfig [-h | --help] [-a] interface

DESCRIPTION//! ## ifconfig Utility for Redox OS

This program implements a basic `ifconfig` utility for Redox OS. It allows users to:

* **Display information** about network interfaces on the system.
* **List all interfaces** with their IP addresses, netmasks, and (placeholder) MAC addresses.
* **Display details** for a specific interface provided as an argument.

The program supports the following options:

* `-h` or `--help`: Prints the help message and exits.
* `-a`: Shows information about all available interfaces.

**Limitations:**

* Currently, the program cannot configure network interfaces (work in progress).
* The displayed MAC address is a placeholder (Redox OS implementation might differ).

**Dependencies:**

* This program uses the `regex` crate for parsing IP addresses and netmasks.

**Usage:**


    Displays and/or configures network interfaces.

OPTIONS
    -h
    --help
        Print this manual page.
    -a
        Display information about all available interfaces in the system.
        interface
        This parameter is a string of the form "name unit", for example "eth0".

AUTHOR
    Written by G. Gielly.
"#; /* @MANEND */

fn main() {
    // Collect command-line arguments, skipping the program name
    let args = env::args().skip(1);
    let mut show_all = false;
    let mut interface_name = None;

    // Parse command-line arguments
    for arg in args {
        match arg.as_str() {
            "-h" | "--help" => {
                // Display the manual page
                println!("{MAN_PAGE}");
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
                    eprintln!("Invalid argument: {arg}");
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
                Ok(interface) => println!("{interface}"),
                Err(_) => eprintln!("Error: Interface '{name}' not found."),
            }
        } else {
            // Display all interfaces if no specific name is provided
            match list_all_interfaces() {
                Ok(interfaces) => {
                    if interfaces.is_empty() {
                        println!("No interfaces found.");
                    } else {
                        for interface in interfaces {
                            println!("{interface}");
                            println!(); // Add an empty line between interfaces
                        }
                    }
                }
                Err(e) => eprintln!("Error listing interfaces: {e}"),
            }
        }
    } else if let Some(name) = interface_name {
        // Show details for a specific interface without `-a`
        match NetworkInterface::new(&name) {
            Ok(interface) => println!("{interface}"),
            Err(_) => eprintln!("Error: Interface '{name}' not found."),
        }
    } else {
        // Default behavior: Show all interfaces if no arguments are provided
        match list_all_interfaces() {
            Ok(interfaces) => {
                if interfaces.is_empty() {
                    println!("No interfaces found.");
                } else {
                    for interface in interfaces {
                        println!("{interface}");
                        println!(); // Add an empty line between interfaces
                    }
                }
            }
            Err(e) => eprintln!("Error listing interfaces: {e}"),
        }
    }
}
