use std::{
    error::Error,
    fmt,
    io::{Read, Write},
    time::{Duration, Instant},
    vec::Vec,
};

use heapless::Vec as FixedVec;
use serial_transport::{Connection, Options, TransportError};

use crate::{FrameError, FrameKind, MAGIC, decode_frame, encode, encoded_len};

const DEFAULT_MAX_FRAME: usize = 4096;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OwnedFrame {
    pub kind: FrameKind,
    pub payload: Vec<u8>,
}

#[derive(Debug)]
pub enum HostError {
    Transport(TransportError),
    Io(std::io::Error),
    Frame(FrameError),
    Timeout,
}

impl fmt::Display for HostError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Transport(error) => error.fmt(formatter),
            Self::Io(error) => error.fmt(formatter),
            Self::Frame(error) => write!(formatter, "invalid HIL frame: {error:?}"),
            Self::Timeout => formatter.write_str("timed out waiting for HIL frame"),
        }
    }
}

impl Error for HostError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Transport(error) => Some(error),
            Self::Io(error) => Some(error),
            _ => None,
        }
    }
}

impl From<TransportError> for HostError {
    fn from(error: TransportError) -> Self {
        Self::Transport(error)
    }
}

impl From<std::io::Error> for HostError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<FrameError> for HostError {
    fn from(error: FrameError) -> Self {
        Self::Frame(error)
    }
}

#[derive(Default)]
pub struct StreamDecoder {
    input: Vec<u8>,
}

impl StreamDecoder {
    pub fn push(&mut self, bytes: &[u8]) {
        self.input.extend_from_slice(bytes);
    }

    pub fn clear(&mut self) {
        self.input.clear();
    }

    pub fn next_frame(&mut self) -> Result<Option<OwnedFrame>, FrameError> {
        let Some(start) = find_magic(&self.input) else {
            let keep = magic_prefix_len(&self.input);
            let discard = self.input.len().saturating_sub(keep);
            self.input.drain(..discard);
            return Ok(None);
        };
        if start != 0 {
            self.input.drain(..start);
        }
        let length = match encoded_len(&self.input) {
            Ok(length) => length,
            Err(FrameError::Incomplete) => return Ok(None),
            Err(error) => {
                self.input.drain(..1);
                return Err(error);
            }
        };
        match decode_frame(&self.input[..length]) {
            Ok(frame) => {
                let owned = OwnedFrame {
                    kind: frame.kind,
                    payload: frame.payload.to_vec(),
                };
                self.input.drain(..length);
                Ok(Some(owned))
            }
            Err(error) => {
                self.input.drain(..1);
                Err(error)
            }
        }
    }
}

pub struct Session {
    connection: Connection,
    decoder: StreamDecoder,
}

impl Session {
    pub fn open(options: Options) -> Result<Self, HostError> {
        Ok(Self::from_connection(Connection::open(options)?))
    }

    pub fn from_connection(connection: Connection) -> Self {
        Self {
            connection,
            decoder: StreamDecoder::default(),
        }
    }

    pub fn path(&self) -> &str {
        self.connection.path()
    }

    pub fn send(&mut self, kind: FrameKind, payload: &[u8]) -> Result<(), HostError> {
        write_frame(&mut self.connection, kind, payload)
    }

    pub fn next(&mut self, timeout: Duration) -> Result<OwnedFrame, HostError> {
        let deadline = Instant::now() + timeout;
        loop {
            if let Some(frame) = self.decoder.next_frame()? {
                return Ok(frame);
            }
            if Instant::now() >= deadline {
                return Err(HostError::Timeout);
            }
            let mut chunk = [0_u8; 256];
            match self.connection.read(&mut chunk) {
                Ok(0) => {}
                Ok(count) => self.decoder.push(&chunk[..count]),
                Err(error) if error.kind() == std::io::ErrorKind::TimedOut => {}
                Err(error) => return Err(error.into()),
            }
        }
    }

    pub fn wait_for<F>(
        &mut self,
        timeout: Duration,
        mut predicate: F,
    ) -> Result<OwnedFrame, HostError>
    where
        F: FnMut(&OwnedFrame) -> bool,
    {
        let deadline = Instant::now() + timeout;
        loop {
            let remaining = deadline
                .checked_duration_since(Instant::now())
                .ok_or(HostError::Timeout)?;
            let frame = self.next(remaining)?;
            if predicate(&frame) {
                return Ok(frame);
            }
        }
    }

    pub fn reopen(&mut self, timeout: Duration) -> Result<(), HostError> {
        self.connection.reopen(timeout)?;
        self.decoder.clear();
        Ok(())
    }
}

pub fn write_frame(
    writer: &mut impl Write,
    kind: FrameKind,
    payload: &[u8],
) -> Result<(), HostError> {
    let mut frame = FixedVec::<u8, DEFAULT_MAX_FRAME>::new();
    encode(kind, payload, &mut frame)?;
    writer.write_all(&frame)?;
    writer.flush()?;
    Ok(())
}

fn find_magic(input: &[u8]) -> Option<usize> {
    input
        .windows(MAGIC.len())
        .position(|window| window == MAGIC)
}

fn magic_prefix_len(input: &[u8]) -> usize {
    (1..MAGIC.len())
        .rev()
        .find(|&length| input.ends_with(&MAGIC[..length]))
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(kind: FrameKind, payload: &[u8]) -> Vec<u8> {
        let mut output = FixedVec::<u8, 64>::new();
        encode(kind, payload, &mut output).unwrap();
        output.to_vec()
    }

    #[test]
    fn decodes_partial_and_concatenated_frames() {
        let first = frame(FrameKind::Event, b"one");
        let second = frame(FrameKind::Event, b"two");
        let mut decoder = StreamDecoder::default();
        decoder.push(&first[..5]);
        assert_eq!(decoder.next_frame().unwrap(), None);
        decoder.push(&first[5..]);
        decoder.push(&second);
        assert_eq!(decoder.next_frame().unwrap().unwrap().payload, b"one");
        assert_eq!(decoder.next_frame().unwrap().unwrap().payload, b"two");
    }

    #[test]
    fn resynchronizes_after_noise() {
        let mut decoder = StreamDecoder::default();
        decoder.push(b"console noise");
        decoder.push(&frame(FrameKind::Event, b"ready"));
        assert_eq!(decoder.next_frame().unwrap().unwrap().payload, b"ready");
    }

    #[test]
    fn resynchronizes_after_corrupt_frame() {
        let mut corrupt = frame(FrameKind::Event, b"bad");
        *corrupt.last_mut().unwrap() ^= 1;
        let mut decoder = StreamDecoder::default();
        decoder.push(&corrupt);
        decoder.push(&frame(FrameKind::Event, b"good"));
        assert!(matches!(decoder.next_frame(), Err(FrameError::BadChecksum)));
        assert_eq!(decoder.next_frame().unwrap().unwrap().payload, b"good");
    }
}
