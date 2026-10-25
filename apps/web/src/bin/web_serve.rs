//! Loopback-only static server for the browser bundle.
//!
//! Usage: `gba-native-web-serve --root <dir> [--bind 127.0.0.1:0]
//! [--lifetime-secs N]`. It prints the bound URL on stdout so the caller can
//! wait for it. GET and HEAD are supported; every other method gets 405.

#![forbid(unsafe_code)]

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Duration;

use gba_native_web::serve::{CSP, ServeError, load};

const MAX_REQUEST_BYTES: u64 = 8 * 1024;
const READ_TIMEOUT: Duration = Duration::from_secs(5);

struct Options {
    root: PathBuf,
    bind: SocketAddr,
    lifetime_secs: Option<u64>,
}

fn parse_args() -> Result<Options, String> {
    let mut args = std::env::args().skip(1);
    let mut root = None;
    let mut bind = SocketAddr::from(([127, 0, 0, 1], 0));
    let mut lifetime_secs = None;
    while let Some(argument) = args.next() {
        match argument.as_str() {
            "--root" => root = Some(PathBuf::from(required(&mut args, "--root")?)),
            "--bind" => bind = parse_loopback_address(&required(&mut args, "--bind")?)?,
            "--lifetime-secs" => {
                let value = required(&mut args, "--lifetime-secs")?;
                lifetime_secs = Some(
                    value
                        .parse()
                        .map_err(|_| format!("invalid --lifetime-secs {value:?}"))?,
                );
            }
            "-h" | "--help" => {
                println!(
                    "usage: gba-native-web-serve --root DIR [--bind 127.0.0.1:0] \
                     [--lifetime-secs N]"
                );
                std::process::exit(0);
            }
            other => return Err(format!("unknown argument {other:?}")),
        }
    }
    let root = root.ok_or_else(|| "--root DIR is required".to_owned())?;
    Ok(Options {
        root,
        bind,
        lifetime_secs,
    })
}

fn parse_loopback_address(value: &str) -> Result<SocketAddr, String> {
    let address = value
        .parse::<SocketAddr>()
        .map_err(|_| format!("invalid --bind address {value:?}; use a loopback IP and port"))?;
    if !address.ip().is_loopback() {
        return Err(format!(
            "--bind must use a loopback IP address; refusing {address}"
        ));
    }
    Ok(address)
}

fn required(args: &mut impl Iterator<Item = String>, name: &str) -> Result<String, String> {
    args.next().ok_or_else(|| format!("{name} needs a value"))
}

fn main() -> ExitCode {
    let options = match parse_args() {
        Ok(options) => options,
        Err(message) => {
            eprintln!("gba-native-web-serve: {message}");
            return ExitCode::from(2);
        }
    };
    if !options.root.is_dir() {
        eprintln!(
            "gba-native-web-serve: bundle root {} is not a directory",
            options.root.display()
        );
        return ExitCode::FAILURE;
    }
    let listener = match TcpListener::bind(options.bind) {
        Ok(listener) => listener,
        Err(error) => {
            eprintln!(
                "gba-native-web-serve: cannot bind {}: {error}",
                options.bind
            );
            return ExitCode::FAILURE;
        }
    };
    let address = match listener.local_addr() {
        Ok(address) => address,
        Err(error) => {
            eprintln!("gba-native-web-serve: cannot read bound address: {error}");
            return ExitCode::FAILURE;
        }
    };
    println!("listening on http://{address}");
    let _ = std::io::stdout().flush();

    if let Some(seconds) = options.lifetime_secs.filter(|seconds| *seconds > 0) {
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_secs(seconds));
            std::process::exit(0);
        });
    }

    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                let root = options.root.clone();
                std::thread::spawn(move || {
                    let _ = handle(stream, &root);
                });
            }
            Err(error) => eprintln!("gba-native-web-serve: accept failed: {error}"),
        }
    }
    ExitCode::SUCCESS
}

fn handle(mut stream: TcpStream, root: &Path) -> std::io::Result<()> {
    let _ = stream.set_read_timeout(Some(READ_TIMEOUT));
    let _ = stream.set_write_timeout(Some(READ_TIMEOUT));
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut request_line = String::new();
    reader
        .by_ref()
        .take(MAX_REQUEST_BYTES)
        .read_line(&mut request_line)?;
    let parts: Vec<&str> = request_line.split_whitespace().collect();
    let (method, target) = match parts.as_slice() {
        [method, target, version] if version.starts_with("HTTP/1.") => (*method, *target),
        _ => return respond_text(&mut stream, 400, "invalid request line"),
    };
    if method != "GET" && method != "HEAD" {
        return respond_text(&mut stream, 405, "only GET and HEAD are supported");
    }
    match load(root, target) {
        Ok(response) => {
            let mut head = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: {}\r\nContent-Length: {}\r\n\
                 Cache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\n\
                 Referrer-Policy: no-referrer\r\nContent-Security-Policy: {}\r\n\
                 Connection: close\r\n\r\n",
                response.content_type,
                response.body.len(),
                CSP
            );
            stream.write_all(head.as_bytes())?;
            head.clear();
            if method == "GET" {
                stream.write_all(&response.body)?;
            }
            stream.flush()
        }
        Err(ServeError::NotFound) => respond_text(&mut stream, 404, "not found"),
        Err(ServeError::Forbidden) | Err(ServeError::TooLarge) => {
            respond_text(&mut stream, 403, "forbidden")
        }
        Err(ServeError::Io(message)) => {
            respond_text(&mut stream, 500, &format!("server error: {message}"))
        }
    }
}

fn respond_text(stream: &mut TcpStream, status: u16, message: &str) -> std::io::Result<()> {
    let reason = match status {
        400 => "Bad Request",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        _ => "Internal Server Error",
    };
    let body = format!("{message}\n");
    write!(
        stream,
        "HTTP/1.1 {status} {reason}\r\nContent-Type: text/plain; charset=utf-8\r\n\
         Content-Length: {}\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\n\
         Connection: close\r\n\r\n{body}",
        body.len()
    )?;
    stream.flush()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bind_accepts_only_explicit_loopback_socket_addresses() {
        assert_eq!(
            parse_loopback_address("127.0.0.1:0").unwrap(),
            SocketAddr::from(([127, 0, 0, 1], 0))
        );
        assert_eq!(
            parse_loopback_address("[::1]:8080").unwrap(),
            "[::1]:8080".parse::<SocketAddr>().unwrap()
        );
        for rejected in [
            "0.0.0.0:8080",
            "192.0.2.1:8080",
            "[::]:8080",
            "[2001:db8::1]:8080",
            "localhost:8080",
        ] {
            assert!(
                parse_loopback_address(rejected).is_err(),
                "accepted unsafe bind address {rejected}"
            );
        }
    }
}
