#![no_std]

//! Generic framing and host sessions for hardware-in-the-loop tests.

use crc::{CRC_32_ISO_HDLC, Crc};
use heapless::Vec;

#[cfg(any(feature = "host", test))]
extern crate std;

#[cfg(feature = "host")]
pub mod host;

pub const MAGIC: &[u8; 4] = b"HIL1";
pub const HEADER_LEN: usize = 11;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum FrameKind {
    Request = 1,
    Response = 2,
    Event = 3,
    Control = 4,
}

impl TryFrom<u8> for FrameKind {
    type Error = FrameError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            1 => Ok(Self::Request),
            2 => Ok(Self::Response),
            3 => Ok(Self::Event),
            4 => Ok(Self::Control),
            _ => Err(FrameError::UnknownKind),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Frame<'a> {
    pub kind: FrameKind,
    pub payload: &'a [u8],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FrameError {
    Incomplete,
    BadMagic,
    UnknownKind,
    TooLarge,
    BadChecksum,
}

pub fn encode<const N: usize>(
    kind: FrameKind,
    payload: &[u8],
    output: &mut Vec<u8, N>,
) -> Result<(), FrameError> {
    output.clear();
    let length = u16::try_from(payload.len()).map_err(|_| FrameError::TooLarge)?;
    output
        .extend_from_slice(MAGIC)
        .map_err(|_| FrameError::TooLarge)?;
    output.push(kind as u8).map_err(|_| FrameError::TooLarge)?;
    output
        .extend_from_slice(&length.to_le_bytes())
        .map_err(|_| FrameError::TooLarge)?;
    let checksum = Crc::<u32>::new(&CRC_32_ISO_HDLC).checksum(payload);
    output
        .extend_from_slice(&checksum.to_le_bytes())
        .map_err(|_| FrameError::TooLarge)?;
    output
        .extend_from_slice(payload)
        .map_err(|_| FrameError::TooLarge)
}

pub fn decode_frame(frame: &[u8]) -> Result<Frame<'_>, FrameError> {
    let length = encoded_len(frame)?;
    let payload = &frame[HEADER_LEN..length];
    let expected = u32::from_le_bytes(
        frame[7..11]
            .try_into()
            .map_err(|_| FrameError::Incomplete)?,
    );
    let actual = Crc::<u32>::new(&CRC_32_ISO_HDLC).checksum(payload);
    if expected != actual {
        return Err(FrameError::BadChecksum);
    }
    Ok(Frame {
        kind: FrameKind::try_from(frame[4])?,
        payload,
    })
}

pub fn encoded_len(frame: &[u8]) -> Result<usize, FrameError> {
    if frame.len() < MAGIC.len() {
        return Err(FrameError::Incomplete);
    }
    if &frame[..MAGIC.len()] != MAGIC {
        return Err(FrameError::BadMagic);
    }
    if frame.len() < HEADER_LEN {
        return Err(FrameError::Incomplete);
    }
    let payload_len = u16::from_le_bytes([frame[5], frame[6]]) as usize;
    let length = HEADER_LEN + payload_len;
    if frame.len() < length {
        return Err(FrameError::Incomplete);
    }
    Ok(length)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_every_kind() {
        for kind in [
            FrameKind::Request,
            FrameKind::Response,
            FrameKind::Event,
            FrameKind::Control,
        ] {
            let mut encoded = Vec::<u8, 64>::new();
            encode(kind, b"payload", &mut encoded).unwrap();
            assert_eq!(
                decode_frame(&encoded),
                Ok(Frame {
                    kind,
                    payload: b"payload"
                })
            );
        }
    }

    #[test]
    fn rejects_corruption() {
        let mut encoded = Vec::<u8, 64>::new();
        encode(FrameKind::Event, b"ready", &mut encoded).unwrap();
        *encoded.last_mut().unwrap() ^= 1;
        assert_eq!(decode_frame(&encoded), Err(FrameError::BadChecksum));
    }

    #[test]
    fn reports_partial_frames() {
        let mut encoded = Vec::<u8, 64>::new();
        encode(FrameKind::Event, b"ready", &mut encoded).unwrap();
        assert_eq!(decode_frame(&encoded[..8]), Err(FrameError::Incomplete));
    }

    #[test]
    fn rejects_unknown_kinds() {
        let mut encoded = Vec::<u8, 64>::new();
        encode(FrameKind::Event, b"ready", &mut encoded).unwrap();
        encoded[4] = 99;
        assert_eq!(decode_frame(&encoded), Err(FrameError::UnknownKind));
    }

    #[test]
    fn rejects_payloads_larger_than_the_wire_length() {
        let payload = std::vec![0; usize::from(u16::MAX) + 1];
        let mut encoded = Vec::<u8, 16>::new();
        assert_eq!(
            encode(FrameKind::Request, &payload, &mut encoded),
            Err(FrameError::TooLarge)
        );
    }
}
