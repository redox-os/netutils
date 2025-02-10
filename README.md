# Redox OS userspace network utilities
This repository contains the network utilities for Redox OS.

[![MIT licensed](https://img.shields.io/badge/license-MIT-blue.svg)](./LICENSE)

## How To Contribute
To learn how to contribute to this system component you need to read the following document :
- [CONTRIBUTING.md](https://gitlab.redox-os.org/redox-os/redox/-/blob/master/CONTRIBUTING.md)

## Development
To learn how to do development with this system component inside the Redox build system you need to read the [Build System](https://doc.redox-os.org/book/build-system-reference.html) and [Coding and Building](https://doc.redox-os.org/book/coding-and-building.html) pages.

## How To Build
To build this system component you need to download the Redox build system, you can learn how to do it on the [Building Redox](https://doc.redox-os.org/book/podman-build.html) page.

This is necessary because they only work with cross-compilation to a Redox virtual machine, but you can do some testing from Linux.

## Network Utilities
- `dhcp`: DHCP client 
- `dns`: simple DNS resolution tool
- `ifconfig`: network interface configuration utility
- `nc`: netcat utility
- `irc`: IRC client 
- `httpd`: simple web server
- `whois`: simple whois client
- `telnetd`: simple telnet server
- `ntp` : NTP client
- `wget`: clone like wget
- `ping` : ping utility

## Nestack
The netstack is based on smoltcp. 






