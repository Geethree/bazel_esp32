//! Shared host-side serial discovery and reconnect handling.

use std::{
    error::Error,
    fmt,
    io::{Read, Write},
    thread,
    time::{Duration, Instant},
};

use serialport::{SerialPort, SerialPortInfo, SerialPortType};

pub const ESPRESSIF_USB_VID: u16 = 0x303a;
const RECONNECT_INTERVAL: Duration = Duration::from_millis(250);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Options {
    pub port: Option<String>,
    pub baud_rate: u32,
    pub timeout: Duration,
    pub usb_vid: Option<u16>,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            port: None,
            baud_rate: 115_200,
            timeout: Duration::from_millis(250),
            usb_vid: Some(ESPRESSIF_USB_VID),
        }
    }
}

#[derive(Debug)]
pub enum TransportError {
    NoDevice,
    MultipleDevices,
    Serial(serialport::Error),
}

impl fmt::Display for TransportError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoDevice => formatter.write_str("no matching USB serial port was found"),
            Self::MultipleDevices => {
                formatter.write_str("multiple matching serial ports found; specify a port")
            }
            Self::Serial(error) => error.fmt(formatter),
        }
    }
}

impl Error for TransportError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Serial(error) => Some(error),
            _ => None,
        }
    }
}

impl From<serialport::Error> for TransportError {
    fn from(error: serialport::Error) -> Self {
        Self::Serial(error)
    }
}

pub struct Connection {
    options: Options,
    path: String,
    port: Option<Box<dyn SerialPort>>,
}

impl Connection {
    pub fn open(options: Options) -> Result<Self, TransportError> {
        let path = discover(&options)?;
        let port = open_port(&path, &options)?;
        Ok(Self {
            options,
            path,
            port: Some(port),
        })
    }

    pub fn path(&self) -> &str {
        &self.path
    }

    pub fn try_clone(&self) -> Result<Box<dyn SerialPort>, TransportError> {
        self.port
            .as_ref()
            .expect("connection always owns an open port")
            .try_clone()
            .map_err(Into::into)
    }

    pub fn reopen(&mut self, timeout: Duration) -> Result<(), TransportError> {
        drop(self.port.take());
        let deadline = Instant::now() + timeout;
        loop {
            if self.options.port.is_none()
                && let Ok(path) = discover(&self.options)
            {
                self.path = path;
            }
            match open_port(&self.path, &self.options) {
                Ok(port) => {
                    self.port = Some(port);
                    return Ok(());
                }
                Err(error) => match retry_delay(Instant::now(), deadline) {
                    Some(delay) => {
                        let _ = error;
                        thread::sleep(delay);
                    }
                    None => return Err(error),
                },
            }
        }
    }
}

impl Read for Connection {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        self.port
            .as_mut()
            .expect("connection always owns an open port")
            .read(buffer)
    }
}

impl Write for Connection {
    fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
        self.port
            .as_mut()
            .expect("connection always owns an open port")
            .write(buffer)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.port
            .as_mut()
            .expect("connection always owns an open port")
            .flush()
    }
}

pub fn discover(options: &Options) -> Result<String, TransportError> {
    if let Some(path) = &options.port {
        return Ok(path.clone());
    }
    let ports = serialport::available_ports()?
        .into_iter()
        .filter(|port| matches_vid(port, options.usb_vid))
        .map(|port| port.port_name)
        .collect();
    select_port(ports)
}

fn matches_vid(port: &SerialPortInfo, usb_vid: Option<u16>) -> bool {
    match (&port.port_type, usb_vid) {
        (SerialPortType::UsbPort(info), Some(expected)) => info.vid == expected,
        (SerialPortType::UsbPort(_), None) => true,
        _ => false,
    }
}

fn open_port(path: &str, options: &Options) -> Result<Box<dyn SerialPort>, TransportError> {
    serialport::new(path, options.baud_rate)
        .timeout(options.timeout)
        .open()
        .map_err(Into::into)
}

fn retry_delay(now: Instant, deadline: Instant) -> Option<Duration> {
    deadline
        .checked_duration_since(now)
        .filter(|remaining| !remaining.is_zero())
        .map(|remaining| remaining.min(RECONNECT_INTERVAL))
}

fn select_port(ports: Vec<String>) -> Result<String, TransportError> {
    let callout_ports: Vec<_> = ports
        .iter()
        .filter(|port| {
            port.rsplit_once('/')
                .map_or(port.as_str(), |(_, name)| name)
                .starts_with("cu.")
        })
        .collect();
    let selected = if callout_ports.is_empty() {
        ports.iter().collect()
    } else {
        callout_ports
    };
    match selected.as_slice() {
        [port] => Ok((*port).clone()),
        [] => Err(TransportError::NoDevice),
        _ => Err(TransportError::MultipleDevices),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_port_does_not_require_discovery() {
        let options = Options {
            port: Some("/dev/test".to_owned()),
            ..Options::default()
        };
        assert_eq!(discover(&options).unwrap(), "/dev/test");
    }

    #[test]
    fn macos_callout_port_wins_over_tty_alias() {
        assert_eq!(
            select_port(vec![
                "/dev/cu.usbmodem101".to_owned(),
                "/dev/tty.usbmodem101".to_owned(),
            ])
            .unwrap(),
            "/dev/cu.usbmodem101"
        );
    }

    #[test]
    fn multiple_boards_are_rejected() {
        assert!(matches!(
            select_port(vec![
                "/dev/cu.usbmodem101".to_owned(),
                "/dev/cu.usbmodem201".to_owned(),
            ]),
            Err(TransportError::MultipleDevices)
        ));
    }

    #[test]
    fn reconnect_retries_are_bounded_by_the_deadline() {
        let now = Instant::now();
        assert_eq!(
            retry_delay(now, now + Duration::from_secs(1)),
            Some(RECONNECT_INTERVAL)
        );
        assert_eq!(
            retry_delay(now, now + Duration::from_millis(10)),
            Some(Duration::from_millis(10))
        );
        assert_eq!(retry_delay(now, now), None);
    }
}
