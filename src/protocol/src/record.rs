use crc::{CRC_32_ISO_HDLC, Crc, NoTable};

use crate::DeviceToken;

/// Length of a committed EEPROM identity record in bytes.
pub const EEPROM_RECORD_LEN: usize = 24;
const HEADER: [u8; 4] = [b'G', b'H', 1, 0xa5];
const CRC: Crc<u32, NoTable> = Crc::<u32, NoTable>::new(&CRC_32_ISO_HDLC);

/// A record is uncommitted, has an unsupported header, or has a bad checksum.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RecordError {
    InvalidHeader,
    InvalidChecksum,
    InvalidLength,
}

const fn checksum(token: &[u8]) -> u32 {
    let mut digest = CRC.digest();
    let (covered_header, _commit_marker) = HEADER.split_at(3);
    digest.update(covered_header);
    digest.update(token);
    digest.finalize()
}

/// Encodes a committed identity record with a CRC-32/ISO-HDLC checksum.
///
/// The checksum detects accidental corruption; it does not authenticate identity.
#[must_use]
pub const fn encode_record(token: DeviceToken) -> [u8; EEPROM_RECORD_LEN] {
    let mut record = [0; EEPROM_RECORD_LEN];
    let (header, rest) = record.split_at_mut(4);
    let (bytes, crc) = rest.split_at_mut(16);
    header.copy_from_slice(&HEADER);
    bytes.copy_from_slice(token.as_bytes());
    crc.copy_from_slice(&checksum(token.as_bytes()).to_le_bytes());
    record
}

/// Validates and decodes one complete committed identity record.
///
/// # Errors
/// Rejects an invalid or uncommitted header and a checksum mismatch.
pub fn decode_record(record: &[u8; EEPROM_RECORD_LEN]) -> Result<DeviceToken, RecordError> {
    let (header, rest) = record.split_at(4);
    let (bytes, crc) = rest.split_at(16);
    if header != HEADER {
        return Err(RecordError::InvalidHeader);
    }
    if crc != checksum(bytes).to_le_bytes() {
        return Err(RecordError::InvalidChecksum);
    }
    let token = bytes
        .try_into()
        .map_err(|_length| RecordError::InvalidLength)?;
    Ok(DeviceToken::new(token))
}
