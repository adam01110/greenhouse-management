use crc::{CRC_32_ISO_HDLC, Crc, NoTable};
use greenhouse_protocol::{
    DeviceToken, EEPROM_RECORD_LEN, RecordError, decode_record, encode_record,
};

const TOKEN: [u8; 16] = [
    0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef, 0xfe, 0xdc, 0xba, 0x98, 0x76, 0x54, 0x32, 0x10,
];
const GOLDEN: [u8; EEPROM_RECORD_LEN] = [
    0x47, 0x48, 0x01, 0xa5, 0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef, 0xfe, 0xdc, 0xba, 0x98,
    0x76, 0x54, 0x32, 0x10, 0x2b, 0x96, 0x71, 0x38,
];

const ENCODED: [u8; EEPROM_RECORD_LEN] = encode_record(DeviceToken::new(TOKEN));

#[test]
fn crc_standard_check() {
    let crc = Crc::<u32, NoTable>::new(&CRC_32_ISO_HDLC);
    assert_eq!(crc.checksum(b"123456789"), 0xcbf4_3926);
}

#[test]
fn golden_record_and_little_endian_checksum() {
    let token = DeviceToken::new(TOKEN);
    assert_eq!(EEPROM_RECORD_LEN, 24);
    assert_eq!(ENCODED, GOLDEN);
    assert_eq!(decode_record(&GOLDEN), Ok(token));
}

#[test]
fn token_roundtrips_include_zero_ff_and_mixed() {
    for bytes in [[0; 16], [0xff; 16], TOKEN] {
        let token = DeviceToken::new(bytes);
        assert_eq!(decode_record(&encode_record(token)), Ok(token));
    }
}

#[test]
fn every_single_bit_corruption_is_rejected() {
    for (index, _) in GOLDEN.iter().enumerate() {
        for bit in 0..8 {
            let mut record = GOLDEN;
            *record.get_mut(index).expect("record index") ^= 1 << bit;
            assert!(decode_record(&record).is_err(), "byte {index}, bit {bit}");
        }
    }
}

#[test]
fn rejects_uncommitted_erased_and_partial_records() {
    assert_eq!(
        decode_record(&[0; EEPROM_RECORD_LEN]),
        Err(RecordError::InvalidHeader)
    );
    assert_eq!(
        decode_record(&[0xff; EEPROM_RECORD_LEN]),
        Err(RecordError::InvalidHeader)
    );
    for offset in 0..4 {
        let mut record = GOLDEN;
        *record.get_mut(offset).expect("header offset") = 0;
        assert_eq!(
            decode_record(&record),
            Err(RecordError::InvalidHeader),
            "header {offset}"
        );
    }
    let mut incomplete = GOLDEN;
    incomplete
        .get_mut(10..20)
        .expect("payload range")
        .fill(0xff);
    assert_eq!(
        decode_record(&incomplete),
        Err(RecordError::InvalidChecksum)
    );
    let mut incomplete_checksum = GOLDEN;
    incomplete_checksum
        .get_mut(20..24)
        .expect("checksum range")
        .fill(0xff);
    assert_eq!(
        decode_record(&incomplete_checksum),
        Err(RecordError::InvalidChecksum)
    );
}

#[test]
fn rejects_wrong_endianness_and_paired_token_bit_flips() {
    let mut wrong_endian = GOLDEN;
    wrong_endian
        .get_mut(20..24)
        .expect("checksum range")
        .reverse();
    assert_eq!(
        decode_record(&wrong_endian),
        Err(RecordError::InvalidChecksum)
    );
    for (first, second) in [(4, 12), (4, 19), (4, 20), (19, 23)] {
        for bit in 0..8 {
            let mut paired = GOLDEN;
            *paired.get_mut(first).expect("first covered byte") ^= 1 << bit;
            *paired.get_mut(second).expect("second covered byte") ^= 1 << bit;
            assert_eq!(
                decode_record(&paired),
                Err(RecordError::InvalidChecksum),
                "paired bytes {first}/{second}, bit {bit}"
            );
        }
    }
}

#[test]
fn valid_recomputed_crc_is_not_authentication() {
    let original = DeviceToken::new(TOKEN);
    let mut altered = TOKEN;
    *altered.first_mut().expect("first token byte") ^= 1;
    let other = DeviceToken::new(altered);
    let forged = encode_record(other);
    assert_ne!(other, original);
    assert_eq!(decode_record(&forged), Ok(other));
}
