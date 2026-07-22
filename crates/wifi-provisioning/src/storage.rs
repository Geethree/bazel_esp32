use crc::{CRC_32_ISO_HDLC, Crc};
use embedded_storage::nor_flash::{NorFlash, ReadNorFlash};

use crate::{Authentication, Credentials};

const MAGIC: &[u8; 4] = b"WFP1";
const VERSION: u8 = 1;
const RECORD_LEN: usize = 128;
const SECTOR_LEN: u32 = 4096;
const CRC_OFFSET: usize = 108;
const SSID_OFFSET: usize = 12;
const PASSWORD_OFFSET: usize = 44;

pub trait CredentialStore {
    type Error;

    fn load(&mut self) -> Result<Option<Credentials>, Self::Error>;
    fn save(&mut self, credentials: &Credentials) -> Result<(), Self::Error>;
    fn clear(&mut self) -> Result<(), Self::Error>;
}

#[derive(Debug, PartialEq, Eq)]
pub enum StoreError<E> {
    Flash(E),
    InvalidRegion,
    VerificationFailed,
}

pub struct FlashJournal<S> {
    storage: S,
    offset: u32,
}

impl<S> FlashJournal<S> {
    pub fn new(storage: S, offset: u32) -> Self {
        Self { storage, offset }
    }

    pub fn into_inner(self) -> S {
        self.storage
    }
}

#[derive(Clone)]
struct DecodedRecord {
    generation: u32,
    credentials: Credentials,
}

impl<S> FlashJournal<S>
where
    S: ReadNorFlash,
{
    fn read_slot(&mut self, slot: usize) -> Result<Option<DecodedRecord>, StoreError<S::Error>> {
        let mut record = [0xff; RECORD_LEN];
        self.storage
            .read(self.offset + slot as u32 * SECTOR_LEN, &mut record)
            .map_err(StoreError::Flash)?;
        Ok(decode(&record))
    }

    fn current(&mut self) -> Result<Option<(usize, DecodedRecord)>, StoreError<S::Error>> {
        let first = self.read_slot(0)?;
        let second = self.read_slot(1)?;
        Ok(match (first, second) {
            (None, None) => None,
            (Some(record), None) => Some((0, record)),
            (None, Some(record)) => Some((1, record)),
            (Some(first), Some(second)) => {
                if is_newer(second.generation, first.generation) {
                    Some((1, second))
                } else {
                    Some((0, first))
                }
            }
        })
    }
}

impl<S> CredentialStore for FlashJournal<S>
where
    S: NorFlash,
{
    type Error = StoreError<S::Error>;

    fn load(&mut self) -> Result<Option<Credentials>, Self::Error> {
        Ok(self.current()?.map(|(_, record)| record.credentials))
    }

    fn save(&mut self, credentials: &Credentials) -> Result<(), Self::Error> {
        if !self.offset.is_multiple_of(SECTOR_LEN) {
            return Err(StoreError::InvalidRegion);
        }

        let current = self.current()?;
        let (slot, generation) = match current {
            Some((slot, record)) => (1 - slot, record.generation.wrapping_add(1)),
            None => (0, 0),
        };
        let address = self.offset + slot as u32 * SECTOR_LEN;
        let record = encode(credentials, generation);
        self.storage
            .erase(address, address + SECTOR_LEN)
            .map_err(StoreError::Flash)?;
        self.storage
            .write(address, &record)
            .map_err(StoreError::Flash)?;

        let mut verification = [0xff; RECORD_LEN];
        self.storage
            .read(address, &mut verification)
            .map_err(StoreError::Flash)?;
        let decoded = decode(&verification).ok_or(StoreError::VerificationFailed)?;
        if decoded.generation != generation || decoded.credentials != *credentials {
            return Err(StoreError::VerificationFailed);
        }
        Ok(())
    }

    fn clear(&mut self) -> Result<(), Self::Error> {
        if !self.offset.is_multiple_of(SECTOR_LEN) {
            return Err(StoreError::InvalidRegion);
        }
        self.storage
            .erase(self.offset, self.offset + SECTOR_LEN * 2)
            .map_err(StoreError::Flash)
    }
}

fn encode(credentials: &Credentials, generation: u32) -> [u8; RECORD_LEN] {
    let mut record = [0xff; RECORD_LEN];
    record[..4].copy_from_slice(MAGIC);
    record[4] = VERSION;
    record[5] = credentials.ssid().len() as u8;
    record[6] = credentials.password().len() as u8;
    record[7] = credentials.authentication() as u8;
    record[8..12].copy_from_slice(&generation.to_le_bytes());
    record[SSID_OFFSET..SSID_OFFSET + credentials.ssid().len()]
        .copy_from_slice(credentials.ssid().as_bytes());
    record[PASSWORD_OFFSET..PASSWORD_OFFSET + credentials.password().len()]
        .copy_from_slice(credentials.password().as_bytes());
    let checksum = Crc::<u32>::new(&CRC_32_ISO_HDLC).checksum(&record[..CRC_OFFSET]);
    record[CRC_OFFSET..CRC_OFFSET + 4].copy_from_slice(&checksum.to_le_bytes());
    record
}

fn decode(record: &[u8; RECORD_LEN]) -> Option<DecodedRecord> {
    if &record[..4] != MAGIC || record[4] != VERSION {
        return None;
    }
    let expected = u32::from_le_bytes(record[CRC_OFFSET..CRC_OFFSET + 4].try_into().ok()?);
    let actual = Crc::<u32>::new(&CRC_32_ISO_HDLC).checksum(&record[..CRC_OFFSET]);
    if expected != actual {
        return None;
    }

    let ssid_len = record[5] as usize;
    let password_len = record[6] as usize;
    if ssid_len == 0 || ssid_len > 32 || password_len > 63 {
        return None;
    }
    let authentication = Authentication::try_from(record[7]).ok()?;
    let ssid = core::str::from_utf8(&record[SSID_OFFSET..SSID_OFFSET + ssid_len]).ok()?;
    let password =
        core::str::from_utf8(&record[PASSWORD_OFFSET..PASSWORD_OFFSET + password_len]).ok()?;
    let credentials = Credentials::new(ssid, password).ok()?;
    if credentials.authentication() != authentication {
        return None;
    }
    Some(DecodedRecord {
        generation: u32::from_le_bytes(record[8..12].try_into().ok()?),
        credentials,
    })
}

fn is_newer(candidate: u32, current: u32) -> bool {
    let distance = candidate.wrapping_sub(current);
    distance != 0 && distance < (1 << 31)
}

#[cfg(test)]
mod tests {
    use super::*;
    use embedded_storage::nor_flash::{ErrorType, NorFlashError, NorFlashErrorKind};

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    struct MemoryError;

    impl NorFlashError for MemoryError {
        fn kind(&self) -> NorFlashErrorKind {
            NorFlashErrorKind::Other
        }
    }

    struct MemoryFlash([u8; 8192]);

    impl Default for MemoryFlash {
        fn default() -> Self {
            Self([0xff; 8192])
        }
    }

    impl ErrorType for MemoryFlash {
        type Error = MemoryError;
    }

    impl ReadNorFlash for MemoryFlash {
        const READ_SIZE: usize = 1;

        fn read(&mut self, offset: u32, bytes: &mut [u8]) -> Result<(), Self::Error> {
            bytes.copy_from_slice(&self.0[offset as usize..offset as usize + bytes.len()]);
            Ok(())
        }

        fn capacity(&self) -> usize {
            self.0.len()
        }
    }

    impl NorFlash for MemoryFlash {
        const WRITE_SIZE: usize = 1;
        const ERASE_SIZE: usize = 4096;

        fn erase(&mut self, from: u32, to: u32) -> Result<(), Self::Error> {
            self.0[from as usize..to as usize].fill(0xff);
            Ok(())
        }

        fn write(&mut self, offset: u32, bytes: &[u8]) -> Result<(), Self::Error> {
            for (destination, source) in self.0[offset as usize..].iter_mut().zip(bytes) {
                *destination &= *source;
            }
            Ok(())
        }
    }

    #[test]
    fn saves_replaces_and_clears() {
        let mut store = FlashJournal::new(MemoryFlash::default(), 0);
        let first = Credentials::new("one", "password-one").unwrap();
        let second = Credentials::new("two", "password-two").unwrap();
        store.save(&first).unwrap();
        assert_eq!(store.load().unwrap(), Some(first));
        store.save(&second).unwrap();
        assert_eq!(store.load().unwrap(), Some(second));
        store.clear().unwrap();
        assert_eq!(store.load().unwrap(), None);
    }

    #[test]
    fn ignores_corrupt_newest_record() {
        let mut store = FlashJournal::new(MemoryFlash::default(), 0);
        let first = Credentials::new("one", "password-one").unwrap();
        let second = Credentials::new("two", "password-two").unwrap();
        store.save(&first).unwrap();
        store.save(&second).unwrap();
        store.storage.0[4096 + 20] ^= 1;
        assert_eq!(store.load().unwrap(), Some(first));
    }

    #[test]
    fn generation_comparison_handles_wrap() {
        assert!(is_newer(0, u32::MAX));
        assert!(!is_newer(u32::MAX, 0));
    }
}
