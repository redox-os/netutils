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
    Written by Guillaume Gielly.
"#; /* @MANEND */

fn main() {
    let mut args = env::args().skip(1);
    let mut show_all = false;
    let mut interface_name = None;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-h" | "--help" => {
                println!("{}", MAN_PAGE);
                return;
            }
            "-a" => show_all = true,
            _ => {
                if interface_name.is_none() {
                    interface_name = Some(arg);
                } else {
                    eprintln!("Invalid argument: {}", arg);
                    return;
                }
            }
        }
    }

    if show_all {
        match list_all_interfaces() {
            Ok(interfaces) => {
                for interface in interfaces {
                    println!("{}", interface);
                    println!(); // Add an empty line between interfaces
                }
            }
            Err(e) => eprintln!("Error listing interfaces: {}", e),
        }
    } else if let Some(name) = interface_name {
        match NetworkInterface::new(&name) {
            Ok(interface) => println!("{}", interface),
            Err(e) => eprintln!("Error: {}", e),
        }
    } else {
        // Default to listing all interfaces
        match list_all_interfaces() {
            Ok(interfaces) => {
                for interface in interfaces {
                    println!("{}", interface);
                    println!(); // Add an empty line between interfaces
                }
            }
            Err(e) => eprintln!("Error listing interfaces: {}", e),
        }
    }
}
