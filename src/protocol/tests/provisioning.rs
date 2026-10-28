use greenhouse_protocol::{
    CommitError, DeviceToken, EEPROM_RECORD_LEN, IdentityStorage, ProvisionError, ProvisionOutcome,
    ProvisionStage, decode_record, encode_record, provision_identity,
};

const TOKEN: DeviceToken = DeviceToken::new([0x42; 16]);
const MARKER: usize = 3;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Op {
    Read(usize),
    Write(usize, u8),
}

#[derive(Clone, Copy, Debug)]
enum Fault {
    Before,
    After,
    Silent,
    DropWrite,
}

#[derive(Debug, Eq, PartialEq)]
struct IoError;

struct Storage {
    bytes: [u8; EEPROM_RECORD_LEN],
    ops: Vec<Op>,
    fault: Option<(usize, Fault)>,
}

impl Storage {
    const fn invalid() -> Self {
        let mut bytes = [0xff; EEPROM_RECORD_LEN];

        // A valid-looking marker must be invalidated even when the header is bad.
        bytes[MARKER] = 0xa5;

        Self {
            bytes,
            ops: Vec::new(),
            fault: None,
        }
    }

    fn apply_write(
        &mut self,
        offset: usize,
        value: u8,
        fault: Option<Fault>,
    ) -> Result<(), IoError> {
        *self
            .bytes
            .get_mut(offset)
            .expect("record write offset is in bounds") =
            value ^ u8::from(matches!(fault, Some(Fault::Silent)));

        match matches!(fault, Some(Fault::After)) {
            true => Err(IoError),
            false => Ok(()),
        }
    }

    fn fault_now(&self) -> Option<Fault> {
        self.fault
            .filter(|(index, _)| *index == self.ops.len() - 1)
            .map(|(_, fault)| fault)
    }
}

impl IdentityStorage for Storage {
    type Error = IoError;

    fn read_byte(&mut self, offset: usize) -> Result<u8, Self::Error> {
        self.ops.push(Op::Read(offset));

        match self.fault_now() {
            Some(Fault::Before | Fault::After) => Err(IoError),
            Some(Fault::Silent) => Ok(*self
                .bytes
                .get(offset)
                .expect("record read offset is in bounds")
                ^ 1),
            Some(Fault::DropWrite) | None => Ok(*self
                .bytes
                .get(offset)
                .expect("record read offset is in bounds")),
        }
    }

    fn write_byte(&mut self, offset: usize, value: u8) -> Result<(), Self::Error> {
        self.ops.push(Op::Write(offset, value));

        let fault = self.fault_now();

        match fault {
            Some(Fault::Before) => Err(IoError),
            Some(Fault::DropWrite) => Ok(()),
            Some(Fault::After | Fault::Silent) | None => self.apply_write(offset, value, fault),
        }
    }
}

fn successful_trace() -> Vec<Op> {
    let mut storage = Storage::invalid();

    assert_eq!(
        provision_identity(&mut storage, TOKEN),
        Ok(ProvisionOutcome::Provisioned)
    );
    assert_eq!(storage.bytes, encode_record(TOKEN));

    storage.ops
}

fn retry(storage: &mut Storage) {
    let already_valid = decode_record(&storage.bytes) == Ok(TOKEN);

    storage.fault = None;
    storage.ops.clear();

    let expected = match already_valid {
        true => ProvisionOutcome::AlreadyProvisioned,
        false => ProvisionOutcome::Provisioned,
    };

    assert_eq!(provision_identity(storage, TOKEN), Ok(expected));
    assert_eq!(decode_record(&storage.bytes), Ok(TOKEN));
    match already_valid {
        true => assert_eq!(
            storage.ops,
            (0..EEPROM_RECORD_LEN).map(Op::Read).collect::<Vec<_>>()
        ),
        false => assert_eq!(storage.ops, successful_trace()),
    }
}

fn expected_io_error(index: usize) -> ProvisionError<IoError> {
    match index {
        73 => ProvisionError::CommitUnverified(CommitError::Write(IoError)),
        74.. => ProvisionError::CommitUnverified(CommitError::Read(IoError)),
        _ => {
            let stages = [
                (24, ProvisionStage::ReadExisting),
                (25, ProvisionStage::Invalidate),
                (26, ProvisionStage::ConfirmInvalid),
                (49, ProvisionStage::WritePayload),
                (73, ProvisionStage::VerifyStaged),
            ];
            let (_, stage) = stages
                .into_iter()
                .find(|(end, _)| index < *end)
                .expect("precommit operation has an expected stage");

            ProvisionError::Storage {
                stage,
                source: IoError,
            }
        }
    }
}

#[test]
fn marker_is_invalid_until_all_payload_bytes_are_verified() {
    let trace = successful_trace();
    let expected_record = encode_record(TOKEN);
    let mut expected = (0..24).map(Op::Read).collect::<Vec<_>>();

    expected.extend([Op::Write(MARKER, 0), Op::Read(MARKER)]);
    expected.extend(
        expected_record
            .iter()
            .copied()
            .enumerate()
            .filter(|(offset, _)| *offset != MARKER)
            .map(|(offset, byte)| Op::Write(offset, byte)),
    );
    expected.extend((0..24).map(Op::Read));
    expected.push(Op::Write(MARKER, 0xa5));
    expected.extend((0..24).map(Op::Read));

    assert_eq!(trace, expected);
    assert_eq!(trace.len(), 98);

    let mut bytes = Storage::invalid().bytes;

    for (index, op) in trace.iter().enumerate() {
        match *op {
            Op::Write(offset, value) => bytes[offset] = value,
            Op::Read(_) => (),
        }

        match index < 73 {
            true => assert!(
                decode_record(&bytes).is_err(),
                "premature commit at {index}"
            ),
            false => assert_eq!(decode_record(&bytes), Ok(TOKEN)),
        }
    }
}

#[test]
fn every_io_error_stops_immediately_and_can_be_retried() {
    let trace = successful_trace();

    for index in 0..trace.len() {
        for fault in [Fault::Before, Fault::After] {
            let mut storage = Storage::invalid();

            storage.fault = Some((index, fault));

            assert_eq!(
                provision_identity(&mut storage, TOKEN),
                Err(expected_io_error(index)),
                "operation {index}, fault {fault:?}"
            );
            assert_eq!(storage.ops, trace[..=index]);
            match index < 73 {
                true => {
                    decode_record(&storage.bytes).expect_err("record must remain invalid");
                }
                false => assert!(storage.ops.contains(&Op::Write(MARKER, 0xa5))),
            }

            retry(&mut storage);
        }
    }
}

#[test]
fn every_silent_read_or_write_mismatch_is_checked_before_success() {
    let trace = successful_trace();

    for index in 0..trace.len() {
        let mut storage = Storage::invalid();

        storage.fault = Some((index, Fault::Silent));

        let result = provision_identity(&mut storage, TOKEN);

        match index {
            // Corrupting an already-invalid initial record leaves it invalid.
            0..24 => assert_eq!(result, Ok(ProvisionOutcome::Provisioned)),
            24..26 => {
                assert_eq!(
                    result,
                    Err(ProvisionError::Mismatch {
                        stage: ProvisionStage::ConfirmInvalid
                    })
                );
                assert_eq!(storage.ops.len(), 26);
            }
            26..73 => {
                assert_eq!(
                    result,
                    Err(ProvisionError::Mismatch {
                        stage: ProvisionStage::VerifyStaged
                    })
                );
                assert_eq!(storage.ops.len(), 73);
                assert!(!storage.ops.contains(&Op::Write(MARKER, 0xa5)));
            }
            73..98 => assert_eq!(
                result,
                Err(ProvisionError::CommitUnverified(CommitError::Mismatch))
            ),
            _ => panic!("unexpected operation {index}"),
        }

        retry(&mut storage);
    }
}

#[test]
fn existing_valid_identities_are_read_only_for_every_token_byte() {
    for byte in 0..=u8::MAX {
        let existing = DeviceToken::new([byte; 16]);
        let mut storage = Storage {
            bytes: encode_record(existing),
            ops: Vec::new(),
            fault: None,
        };

        assert_eq!(
            provision_identity(&mut storage, existing),
            Ok(ProvisionOutcome::AlreadyProvisioned)
        );
        assert_eq!(storage.ops, (0..24).map(Op::Read).collect::<Vec<_>>());

        storage.ops.clear();

        let different = DeviceToken::new([byte ^ 1; 16]);

        assert_eq!(
            provision_identity(&mut storage, different),
            Err(ProvisionError::IdentityConflict)
        );
        assert_eq!(storage.ops, (0..24).map(Op::Read).collect::<Vec<_>>());
        assert_eq!(storage.bytes, encode_record(existing));
    }
}

#[test]
fn initial_read_error_never_writes_even_for_an_existing_valid_identity() {
    for index in 0..EEPROM_RECORD_LEN {
        let mut storage = Storage {
            bytes: encode_record(TOKEN),
            ops: Vec::new(),
            fault: Some((index, Fault::Before)),
        };

        assert_eq!(
            provision_identity(&mut storage, TOKEN),
            Err(expected_io_error(index))
        );
        assert_eq!(storage.ops, (0..=index).map(Op::Read).collect::<Vec<_>>());
        assert_eq!(storage.bytes, encode_record(TOKEN));
    }
}

#[test]
fn erased_and_each_corrupt_record_byte_can_be_provisioned() {
    let mut records = vec![[0xff; EEPROM_RECORD_LEN], [0; EEPROM_RECORD_LEN]];

    for offset in 0..EEPROM_RECORD_LEN {
        let mut record = encode_record(DeviceToken::new([0x19; 16]));

        record[offset] ^= 1;

        records.push(record);
    }

    for bytes in records {
        decode_record(&bytes).expect_err("fixture must be invalid");

        let mut storage = Storage {
            bytes,
            ops: Vec::new(),
            fault: None,
        };

        assert_eq!(
            provision_identity(&mut storage, TOKEN),
            Ok(ProvisionOutcome::Provisioned)
        );
        assert_eq!(storage.bytes, encode_record(TOKEN));

        retry(&mut storage);
    }
}

#[test]
fn every_silently_dropped_write_is_detected_and_retryable() {
    let trace = successful_trace();

    for (index, op) in trace.iter().enumerate() {
        match op {
            Op::Read(_) => continue,
            Op::Write(_, _) => (),
        }

        let mut storage = Storage::invalid();

        storage.fault = Some((index, Fault::DropWrite));

        let expected = match index {
            24 => ProvisionError::Mismatch {
                stage: ProvisionStage::ConfirmInvalid,
            },
            73 => ProvisionError::CommitUnverified(CommitError::Mismatch),
            _ => ProvisionError::Mismatch {
                stage: ProvisionStage::VerifyStaged,
            },
        };

        assert_eq!(
            provision_identity(&mut storage, TOKEN),
            Err(expected),
            "operation {index}"
        );
        decode_record(&storage.bytes).expect_err("record must remain invalid");

        retry(&mut storage);
    }
}
