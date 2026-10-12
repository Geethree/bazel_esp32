use std::{
    error::Error,
    io::{Read, Write},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
};

#[cfg(feature = "hil")]
use hil_testing::{
    FrameKind,
    host::{HostError, Session, write_frame},
};
use serial_transport::{Connection, Options as SerialOptions};
#[cfg(feature = "hil")]
use std::time::Duration;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    Raw,
    Hil,
}

struct Options {
    mode: Mode,
    serial: SerialOptions,
}

fn main() -> Result<(), Box<dyn Error>> {
    let options = Options::parse(std::env::args().skip(1))?;
    let connection = Connection::open(options.serial)?;
    println!("Attached to {}. Ctrl-D exits.", connection.path());
    match options.mode {
        Mode::Raw => raw_console(connection),
        Mode::Hil => hil_console(connection),
    }
}

fn raw_console(mut connection: Connection) -> Result<(), Box<dyn Error>> {
    let mut reader = connection.try_clone()?;
    let running = Arc::new(AtomicBool::new(true));
    let reader_running = Arc::clone(&running);
    let output = thread::spawn(move || {
        let mut stdout = std::io::stdout().lock();
        let mut buffer = [0_u8; 256];
        while reader_running.load(Ordering::Relaxed) {
            match reader.read(&mut buffer) {
                Ok(0) => {}
                Ok(count) => {
                    if stdout.write_all(&buffer[..count]).is_err() || stdout.flush().is_err() {
                        break;
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::TimedOut => {}
                Err(_) => break,
            }
        }
    });

    connection.write_all(b"\nhelp\n")?;
    connection.flush()?;
    let stdin = std::io::stdin();
    loop {
        let mut command = String::new();
        if stdin.read_line(&mut command)? == 0 {
            break;
        }
        connection.write_all(command.as_bytes())?;
        if !command.ends_with('\n') {
            connection.write_all(b"\n")?;
        }
        connection.flush()?;
    }

    running.store(false, Ordering::Relaxed);
    drop(connection);
    let _ = output.join();
    Ok(())
}

#[cfg(feature = "hil")]
fn hil_console(connection: Connection) -> Result<(), Box<dyn Error>> {
    let mut writer = connection.try_clone()?;
    let running = Arc::new(AtomicBool::new(true));
    let reader_running = Arc::clone(&running);
    let output = thread::spawn(move || {
        let mut session = Session::from_connection(connection);
        while reader_running.load(Ordering::Relaxed) {
            match session.next(Duration::from_millis(250)) {
                Ok(frame) => print_frame(frame.kind, &frame.payload),
                Err(HostError::Timeout) => {}
                Err(error) => {
                    eprintln!("HIL receive error: {error}");
                    break;
                }
            }
        }
    });

    println!("HIL mode. Type `help` for local commands.");
    let stdin = std::io::stdin();
    loop {
        let mut command = String::new();
        if stdin.read_line(&mut command)? == 0 {
            break;
        }
        match parse_hil_command(command.trim_end())? {
            HilCommand::Help => println!(
                "request <escaped-payload>  send request frame\n\
                 control <escaped-payload>  send opaque control frame\n\
                 help                       show commands"
            ),
            HilCommand::Send(kind, payload) => write_frame(&mut writer, kind, &payload)?,
            HilCommand::Empty => {}
        }
    }

    running.store(false, Ordering::Relaxed);
    drop(writer);
    let _ = output.join();
    Ok(())
}

#[cfg(not(feature = "hil"))]
fn hil_console(_connection: Connection) -> Result<(), Box<dyn Error>> {
    Err("this serial-console target was built without the `hil` feature".into())
}

#[cfg(feature = "hil")]
enum HilCommand {
    Empty,
    Help,
    Send(FrameKind, Vec<u8>),
}

#[cfg(feature = "hil")]
fn parse_hil_command(command: &str) -> Result<HilCommand, Box<dyn Error>> {
    if command.is_empty() {
        return Ok(HilCommand::Empty);
    }
    if command == "help" {
        return Ok(HilCommand::Help);
    }
    let (name, payload) = command
        .split_once(' ')
        .ok_or("expected `request <payload>` or `control <payload>`")?;
    let kind = match name {
        "request" => FrameKind::Request,
        "control" => FrameKind::Control,
        _ => return Err("expected `request`, `control`, or `help`".into()),
    };
    Ok(HilCommand::Send(kind, unescape(payload)?))
}

#[cfg(feature = "hil")]
fn unescape(value: &str) -> Result<Vec<u8>, Box<dyn Error>> {
    let mut output = Vec::new();
    let mut bytes = value.bytes();
    while let Some(byte) = bytes.next() {
        if byte != b'\\' {
            output.push(byte);
            continue;
        }
        output.push(match bytes.next().ok_or("trailing escape")? {
            b'n' => b'\n',
            b'r' => b'\r',
            b't' => b'\t',
            b'\\' => b'\\',
            _ => return Err("supported escapes are \\n, \\r, \\t, and \\\\".into()),
        });
    }
    Ok(output)
}

#[cfg(feature = "hil")]
fn print_frame(kind: FrameKind, payload: &[u8]) {
    let label = match kind {
        FrameKind::Request => "request",
        FrameKind::Response => "response",
        FrameKind::Event => "event",
        FrameKind::Control => "control",
    };
    if let Ok(text) = core::str::from_utf8(payload) {
        println!("[{label}] {text}");
    } else {
        print!("[{label}]");
        for byte in payload {
            print!(" {byte:02x}");
        }
        println!();
    }
}

impl Options {
    fn parse(arguments: impl IntoIterator<Item = String>) -> Result<Self, Box<dyn Error>> {
        let mut options = Self {
            mode: Mode::Raw,
            serial: SerialOptions::default(),
        };
        let mut arguments = arguments.into_iter();
        while let Some(flag) = arguments.next() {
            match flag.as_str() {
                "--mode" => {
                    options.mode = match arguments.next().as_deref() {
                        Some("raw") => Mode::Raw,
                        Some("hil") => Mode::Hil,
                        _ => return Err("--mode must be `raw` or `hil`".into()),
                    };
                }
                "--port" => {
                    options.serial.port = Some(arguments.next().ok_or("missing --port value")?);
                }
                "--baud" => {
                    options.serial.baud_rate =
                        arguments.next().ok_or("missing --baud value")?.parse()?;
                }
                _ => return Err(format!("unknown argument: {flag}").into()),
            }
        }
        Ok(options)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_to_raw_mode() {
        let options = Options::parse(Vec::new()).unwrap();
        assert_eq!(options.mode, Mode::Raw);
    }

    #[test]
    fn parses_port_baud_and_mode() {
        let options = Options::parse([
            "--mode".to_owned(),
            "hil".to_owned(),
            "--port".to_owned(),
            "/dev/test".to_owned(),
            "--baud".to_owned(),
            "9600".to_owned(),
        ])
        .unwrap();
        assert_eq!(options.mode, Mode::Hil);
        assert_eq!(options.serial.port.as_deref(), Some("/dev/test"));
        assert_eq!(options.serial.baud_rate, 9600);
    }

    #[cfg(feature = "hil")]
    #[test]
    fn parses_escaped_hil_payload() {
        match parse_hil_command(r"request GET /status HTTP/1.1\r\n\r\n").unwrap() {
            HilCommand::Send(FrameKind::Request, payload) => {
                assert_eq!(payload, b"GET /status HTTP/1.1\r\n\r\n");
            }
            _ => panic!("unexpected command"),
        }
    }
}
