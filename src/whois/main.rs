extern crate arg_parser;

use std::process::exit;
use std::error::Error;
use std::net::TcpStream;
use std::io::{Write, BufRead, BufReader};

fn main() {
    // Setup stderr stream in case of failure. Required by fail()
    let mut stderr = std::io::stderr();

    // Set defaults
    let mut host = "whois.iana.org".to_string();
    let mut port: u16 = 43;
    let query: String;

    // Parse the arguments.
    {
        let mut parser = arg_parser::ArgParser::new(3)
            .add_flag(&["", "help"])
            .add_opt("h", "host")
            .add_opt("p", "port");

        parser.parse(std::env::args());

        if parser.found("help") {
            println!("Usage: whois [(-h | --host) hostname] [(-p | --port) port] query");
            exit(0);
        }

        if let Some(hostname) = parser.get_opt("host") {
            // For easier case insensitive comparisons, lowercase the host.
            host = hostname.to_ascii_lowercase();
        }

        if let Some(port_string) = parser.get_opt("port") {
            match port_string.parse::<u16>() {
                Ok(num) => port = num,
                Err(e) => {
                    fail(
                        format!("failed to parse '{}', {}", port_string, e.description()).as_str(),
                        &mut stderr,
                    )
                }
            }
        }

        query = parser.args.join(" ")
    }

    if query.is_empty() {
        fail("Query is empty", &mut stderr);
    }

    // Remember previous hosts to prevent an infinite loop
    let mut previous_hosts = Vec::with_capacity(1);
    while host != "" {
        let mut nhost = "".to_string();
        // Connect to the whois host
        let connect_result = TcpStream::connect((host.as_str(), port));
        match connect_result {
            Ok(mut stream) => {
                // Send the query. A curfeed and a newline are required by the WHOIS standard.
                if let Err(e) = write!(stream, "{}\r\n", query) {
                    fail(
                        format!("Can't send to {}, {}", host, e.description()).as_str(),
                        &mut stderr,
                    );
                }

                /* Read the response and determine if it's a thick or a thin client. Unfortunately,
                 * there's no reliable way to differentiate between the two. The following method is
                 * borrowed from the FreeBSD whois client. */
                let mut reader = BufReader::new(stream);
                let mut line = String::with_capacity(64);
                loop {
                    match reader.read_line(&mut line) {
                        Ok(0) => break,
                        Ok(_) => {
                            print!("{}", line);
                            let trimmed_line = line.trim_start();
                            if let Some(trimmed_line) =
                                [
                                    "whois:",
                                    "Whois Server:",
                                    "Registrar WHOIS Server:",
                                    "ReferralServer:  whois://",
                                    "descr:          region. Please query",
                                ].iter()
                                    .filter(|&prefix| trimmed_line.starts_with(prefix))
                                    .find_map(|&prefix| trimmed_line.get(prefix.len()..))
                            {
                                nhost = trimmed_line
                                    .trim_start()
                                    .trim_end_matches(|c: char| {
                                        !(c.is_ascii_alphanumeric() || c == '.' || c == '-')
                                    })
                                    .to_ascii_lowercase();

                                //Print the rest of the whois data
                                if let Err(e) = std::io::copy(&mut reader, &mut std::io::stdout()) {
                                    fail(
                                        format!(
                                            "Can't print whois data from {}, {}",
                                            host,
                                            e.description()
                                        ).as_str(),
                                        &mut stderr,
                                    );
                                }
                                break;
                            }
                        }
                        Err(e) => {
                            fail(
                                format!("Can't read from {}, {}", host, e.description()).as_str(),
                                &mut stderr,
                            )
                        }
                    }
                    line.clear();
                }
            }
            Err(e) => {
                fail(
                    format!("Failed to connect to '{}', {}", host, e.description()).as_str(),
                    &mut stderr,
                )
            }
        }

        // Ignore and don't report an error for self-referrals
        if host == nhost {
            break;
        }

        // Check for and prevent referral loops
        {
            let mut previous_hosts_iter = previous_hosts.iter();
            if let Some(_) = previous_hosts_iter.position(|s| *s == nhost) {
                fail(
                    format!(
                        "Detected whois referral loop between hosts:\n{}\n{}",
                        nhost,
                        previous_hosts_iter.as_slice().join("\n")
                    ).as_str(),
                    &mut stderr,
                );
            }
        }

        previous_hosts.push(host.clone());
        host = nhost;
    }
}

/// Print error message to standard error, and exit with code, _1_.
fn fail<'a>(s: &'a str, stderr: &mut std::io::Stderr) -> ! {
    let mut stderr = stderr.lock();

    let _ = stderr.write(b"error: ");
    let _ = stderr.write(s.as_bytes());
    let _ = stderr.write(b"\n");
    let _ = stderr.flush();
    exit(1);
}
