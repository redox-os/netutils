#![deny(warnings)]
#![feature(asm)]
#![feature(const_fn)]

#[cfg(not(target_os = "redox"))]
extern crate libc;

#[cfg(target_os = "redox")]
extern crate syscall;

use std::{env, str};
use std::error::Error;
use std::fs::{File, OpenOptions};
use std::io::{self, ErrorKind, Result, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::os::unix::io::{FromRawFd, IntoRawFd, RawFd};
use std::os::unix::process::CommandExt;
use std::process::{Command, Stdio};
use std::thread;

use getpty::getpty;

mod getpty;

#[cfg(not(target_os="redox"))]
pub fn before_exec() -> Result<()> {
    use libc;
    unsafe {
        libc::setsid();
        libc::ioctl(0, libc::TIOCSCTTY, 1);
    }
    Ok(())
}

#[cfg(target_os="redox")]
pub fn before_exec() -> Result<()> {
    Ok(())
}

#[cfg(target_os = "redox")]
fn handle(stream: &mut TcpStream, master_fd: RawFd) {
    extern crate syscall;

    use std::os::unix::io::AsRawFd;

    let mut event_file = File::open("event:").expect("telnetd: failed to open event file");

    let stream_fd = stream.as_raw_fd();
    syscall::fevent(stream_fd, syscall::flag::EVENT_READ).expect("telnetd: failed to fevent console window");

    let mut master = unsafe { File::from_raw_fd(master_fd) };
    syscall::fevent(master_fd, syscall::flag::EVENT_READ).expect("telnetd: failed to fevent master PTY");

    let mut handle_event = |event_id: usize, event_count: usize| -> bool {
        if event_id == stream_fd {
            let mut inbound = [0; 4096];
            match stream.read(&mut inbound) {
                Ok(count) => if count == 0 {
                    return false;
                } else {
                    master.write(&inbound[..count]).expect("telnetd: failed to write to pty");
                    master.flush().expect("telnetd: failed to flush pty");
                },
                Err(err) => match err.kind() {
                    ErrorKind::WouldBlock => (),
                    _ => panic!("telnetd: failed to read stream: {:?}", err)
                }
            }
        } else if event_id == master_fd {
            let mut outbound = [0; 4096];
            let count = master.read(&mut outbound).expect("telnetd: failed to read master PTY");
            if count == 0 {
                if event_count == 0 {
                    return false;
                }
            } else {
                stream.write(&outbound[1..count]).expect("telnetd: failed to write to stream");
                stream.flush().expect("telnetd: failed to flush stream");
            }
        } else {
            println!("Unknown event {}", event_id);
        }

        true
    };

    handle_event(stream_fd, 0);
    handle_event(master_fd, 0);

    'events: loop {
        let mut sys_event = syscall::Event::default();
        event_file.read(&mut sys_event).expect("telnetd: failed to read event file");
        if ! handle_event(sys_event.id, sys_event.data) {
            break 'events;
        }
    }
}

#[cfg(not(target_os = "redox"))]
fn handle(stream: &mut TcpStream, master_fd: RawFd) {
    use libc;
    use std::time::Duration;

    unsafe {
        let size = libc::winsize {
            ws_row: 30,
            ws_col: 80,
            ws_xpixel: 0,
            ws_ypixel: 0
        };
        libc::ioctl(master_fd, libc::TIOCSWINSZ, &size as *const libc::winsize);
    }

    let mut master = unsafe { File::from_raw_fd(master_fd) };

    loop {
        let mut inbound = [0; 4096];
        match stream.read(&mut inbound) {
            Ok(count) => if count == 0 {
                return;
            } else {
                master.write(&inbound[..count]).expect("telnetd: failed to write to pty");
                master.flush().expect("telnetd: failed to flush pty");
            },
            Err(err) => match err.kind() {
                ErrorKind::WouldBlock => (),
                _ => panic!("telnetd: failed to read stream: {:?}", err)
            }
        }

        let mut outbound = [0; 4096];
        match master.read(&mut outbound) {
            Ok(count) => if count == 0 {
                return;
            } else {
                stream.write(&outbound[1..count]).expect("telnetd: failed to write to stream");
                stream.flush().expect("telnetd: failed to flush stream");
            },
            Err(err) => match err.kind() {
                ErrorKind::WouldBlock => (),
                _ => panic!("telnetd: failed to read master PTY: {:?}", err)
            }
        }

        thread::sleep(Duration::new(0, 100));
    }
}

fn telnet() {
    let listener = TcpListener::bind("0.0.0.0:8023").unwrap();
    loop {
        let (mut stream, address) = listener.accept().unwrap();
        thread::spawn(move || {
            println!("Connection from {} opened", address);

            stream.set_nonblocking(true).expect("telnetd: failed to set nonblocking");

            let (master_fd, tty_path) = getpty();

            let slave_stdin = OpenOptions::new().read(true).write(true).open(&tty_path).unwrap();
            let slave_stdout = OpenOptions::new().read(true).write(true).open(&tty_path).unwrap();
            let slave_stderr = OpenOptions::new().read(true).write(true).open(&tty_path).unwrap();


            env::set_var("COLUMNS", "80");
            env::set_var("LINES", "30");
            env::set_var("TERM", "linux");
            env::set_var("TTY", format!("{}", tty_path.display()));

            match unsafe {
                Command::new("login")
                    .stdin(Stdio::from_raw_fd(slave_stdin.into_raw_fd()))
                    .stdout(Stdio::from_raw_fd(slave_stdout.into_raw_fd()))
                    .stderr(Stdio::from_raw_fd(slave_stderr.into_raw_fd()))
                    .before_exec(|| {
                        before_exec()
                    })
                    .spawn()
            } {
                Ok(mut process) => {
                    handle(&mut stream, master_fd);

                    let _ = process.kill();
                    process.wait().expect("telnetd: failed to wait on shell");

                    println!("Connection from {} closed", address);
                },
                Err(err) => {
                    let term_stderr = io::stderr();
                    let mut term_stderr = term_stderr.lock();
                    let _ = term_stderr.write(b"failed to execute 'login': ");
                    let _ = term_stderr.write(err.description().as_bytes());
                    let _ = term_stderr.write(b"\n");
                }
            }
        });
    }
}

#[cfg(target_os = "redox")]
fn fork()  -> usize {
    extern crate syscall;
    unsafe { syscall::clone(0).unwrap() }
}

#[cfg(not(target_os = "redox"))]
fn fork()  -> usize {
    extern crate libc;
    unsafe { libc::fork() as usize }
}

fn main() {
    let mut background = false;
    for arg in env::args().skip(1) {
        match arg.as_ref() {
            "-b" => background = true,
            _ => ()
        }
    }

    println!("Telnet");
    if background {
        if fork() == 0 {
            telnet();
        }
    } else {
        telnet();
    }
}
