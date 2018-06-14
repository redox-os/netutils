#![deny(warnings)]
#![feature(asm)]
#![feature(const_fn)]

extern crate mio;
extern crate tokio;
extern crate tokio_reactor;

#[cfg(not(target_os = "redox"))]
extern crate libc;

#[cfg(target_os = "redox")]
extern crate syscall;
#[cfg(target_os = "redox")]
extern crate redox_termios;

use mio::{Poll as MioPoll, Token, Ready, PollOpt};
use mio::unix::EventedFd;
use std::env;
use std::error::Error;
use std::fs::{File, OpenOptions};
use std::io::{self, Result, Write};
use std::os::unix::io::{AsRawFd, FromRawFd, IntoRawFd, RawFd};
use std::os::unix::process::CommandExt;
use std::process::{Command, Child, Stdio};
use std::sync::{Arc, Mutex};
use tokio::net::{TcpListener, TcpStream};
use tokio::prelude::*;
use tokio_reactor::PollEvented;

#[cfg(target_os = "redox")]
use redox_termios::Winsize;

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

pub struct EventedPty(File);

impl mio::Evented for EventedPty {
    fn register(&self, poll: &MioPoll, token: Token, interest: Ready, opts: PollOpt) -> io::Result<()> {
        EventedFd(&self.0.as_raw_fd()).register(poll, token, interest, opts)
    }

    fn reregister(&self, poll: &MioPoll, token: Token, interest: Ready, opts: PollOpt) -> io::Result<()> {
        EventedFd(&self.0.as_raw_fd()).reregister(poll, token, interest, opts)
    }

    fn deregister(&self, poll: &MioPoll) -> io::Result<()> {
        EventedFd(&self.0.as_raw_fd()).deregister(poll)
    }
}
impl io::Read for EventedPty {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.0.read(buf)
    }
}
impl io::Write for EventedPty {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.write(buf)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.0.flush()
    }
}

fn handle(stream: TcpStream, master_fd: RawFd, process: Child) {
    #[cfg(not(target_os = "redox"))]
    unsafe {
        let size = libc::winsize {
            ws_row: 30,
            ws_col: 80,
            ws_xpixel: 0,
            ws_ypixel: 0
        };
        libc::ioctl(master_fd, libc::TIOCSWINSZ, &size as *const libc::winsize);
    }
    #[cfg(target_os = "redox")]
    {
        let winsize = syscall::dup(master_fd, b"winsize").expect("failed to get winsize property");
        let size = Winsize {
            ws_row: 30,
            ws_col: 80
        };
        let ret = syscall::write(winsize, &size);
        syscall::close(winsize).expect("failed to close winsize property");
        ret.expect("failed to set winsize property");
    }

    let master = PollEvented::new(EventedPty(unsafe { File::from_raw_fd(master_fd) }));

    let (stream_read, stream_write) = stream.split();
    let (master_read, master_write) = master.split();

    let process = Arc::new(Mutex::new(process));
    let process2 = Arc::clone(&process);

    tokio::spawn(
        tokio::io::copy(stream_read, master_write)
            .map(|_| ())
            .select(tokio::io::copy(master_read, stream_write)
                .map(|_| ()))
            .map(move |_| {
                let mut process = process.lock().unwrap();
                process.kill().expect("failed to kill child process");
                process.wait().expect("failed to wait for child process");
            })
            .map_err(move |err| {
                eprintln!("error reading stream: {}", err.0);
                let mut process = process2.lock().unwrap();
                process.kill().expect("failed to kill child process");
                process.wait().expect("failed to wait for child process");
            }));
}

fn telnet() {
    let addr = "0.0.0.0:8023".parse().unwrap();
    let listener = TcpListener::bind(&addr).unwrap();

    tokio::run(listener.incoming()
        .map_err(|err| eprintln!("accept error: {}", err))
        .for_each(|stream| {
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
                Ok(process) => {
                    handle(stream, master_fd, process);
                },
                Err(err) => {
                    let term_stderr = io::stderr();
                    let mut term_stderr = term_stderr.lock();
                    let _ = term_stderr.write(b"failed to execute 'login': ");
                    let _ = term_stderr.write(err.description().as_bytes());
                    let _ = term_stderr.write(b"\n");
                }
            }

            Ok(())
        }));
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
