use crate::record::COMMIT_MARKER_OFFSET;
use crate::{DeviceToken, EEPROM_RECORD_LEN, decode_record, encode_record};

/// Exclusive access to one identity record, addressed from zero.
/// # Storage contract
/// Each operation must finish before returning. Successful writes must be
/// readable immediately; adapters must wait for any hardware write completion.
/// An error may occur after a write has taken effect. This interface does not
/// guarantee atomic EEPROM writes or protection against concurrent modification.
/// Identity preservation requires reads to return the actual stored byte or an
/// error. A silently corrupted initial read can make a valid identity appear
/// invalid, allowing it to be overwritten; CRC cannot establish read reliability.
pub trait IdentityStorage {
    type Error;

    /// # Errors
    /// Returns the storage failure without substituting a byte.
    fn read_byte(&mut self, offset: usize) -> Result<u8, Self::Error>;

    /// # Errors
    /// Returns the storage failure, even if the byte may already have changed.
    fn write_byte(&mut self, offset: usize, value: u8) -> Result<(), Self::Error>;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProvisionOutcome {
    AlreadyProvisioned,
    Provisioned,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProvisionStage {
    ReadExisting,
    Invalidate,
    ConfirmInvalid,
    WritePayload,
    VerifyStaged,
}

/// The commit was attempted, but a complete verified identity was not observed.
/// The caller must reread storage before deciding whether another attempt is needed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CommitError<E> {
    Write(E),
    Read(E),
    Mismatch,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProvisionError<E> {
    IdentityConflict,
    Storage { stage: ProvisionStage, source: E },
    Mismatch { stage: ProvisionStage },
    CommitUnverified(CommitError<E>),
}

/// Requires reliable initial reads as specified by `IdentityStorage`.
/// No further I/O is attempted after an error, including cleanup or rollback.
/// # Errors
/// Returns a conflict for a different valid identity, storage or readback errors
/// before commit, or `CommitUnverified` once the final marker write is attempted.
pub fn provision_identity<S: IdentityStorage>(
    storage: &mut S,
    token: DeviceToken,
) -> Result<ProvisionOutcome, ProvisionError<S::Error>> {
    if let Some(outcome) = existing_outcome(storage, token)? {
        Ok(outcome)
    } else {
        let expected = encode_record(token);

        stage_record(storage, expected)?;

        commit_record(storage, token, expected[COMMIT_MARKER_OFFSET])
    }
}

fn existing_outcome<S: IdentityStorage>(
    storage: &mut S,
    token: DeviceToken,
) -> Result<Option<ProvisionOutcome>, ProvisionError<S::Error>> {
    let existing = read_record(storage).map_err(|source| ProvisionError::Storage {
        stage: ProvisionStage::ReadExisting,
        source,
    })?;

    decode_record(&existing).map_or_else(
        |_| Ok(None),
        |existing_token| matching_identity(existing_token, token).map(Some),
    )
}

fn matching_identity<E>(
    existing: DeviceToken,
    requested: DeviceToken,
) -> Result<ProvisionOutcome, ProvisionError<E>> {
    match existing == requested {
        true => Ok(ProvisionOutcome::AlreadyProvisioned),
        false => Err(ProvisionError::IdentityConflict),
    }
}

fn stage_record<S: IdentityStorage>(
    storage: &mut S,
    mut expected: [u8; EEPROM_RECORD_LEN],
) -> Result<(), ProvisionError<S::Error>> {
    expected[COMMIT_MARKER_OFFSET] = 0;

    invalidate_record(storage)?;
    write_payload(storage, &expected)?;

    verify_staged(storage, &expected)
}

fn invalidate_record<S: IdentityStorage>(storage: &mut S) -> Result<(), ProvisionError<S::Error>> {
    storage
        .write_byte(COMMIT_MARKER_OFFSET, 0)
        .map_err(|source| ProvisionError::Storage {
            stage: ProvisionStage::Invalidate,
            source,
        })?;

    let marker =
        storage
            .read_byte(COMMIT_MARKER_OFFSET)
            .map_err(|source| ProvisionError::Storage {
                stage: ProvisionStage::ConfirmInvalid,
                source,
            })?;

    match marker == 0 {
        true => Ok(()),
        false => Err(ProvisionError::Mismatch {
            stage: ProvisionStage::ConfirmInvalid,
        }),
    }
}

fn write_payload<S: IdentityStorage>(
    storage: &mut S,
    expected: &[u8; EEPROM_RECORD_LEN],
) -> Result<(), ProvisionError<S::Error>> {
    for (offset, byte) in expected.iter().copied().enumerate() {
        match offset == COMMIT_MARKER_OFFSET {
            true => (),
            false => {
                storage
                    .write_byte(offset, byte)
                    .map_err(|source| ProvisionError::Storage {
                        stage: ProvisionStage::WritePayload,
                        source,
                    })?;
            }
        }
    }

    Ok(())
}

fn verify_staged<S: IdentityStorage>(
    storage: &mut S,
    expected: &[u8; EEPROM_RECORD_LEN],
) -> Result<(), ProvisionError<S::Error>> {
    let staged = read_record(storage).map_err(|source| ProvisionError::Storage {
        stage: ProvisionStage::VerifyStaged,
        source,
    })?;

    match &staged == expected {
        true => Ok(()),
        false => Err(ProvisionError::Mismatch {
            stage: ProvisionStage::VerifyStaged,
        }),
    }
}

fn commit_record<S: IdentityStorage>(
    storage: &mut S,
    token: DeviceToken,
    marker: u8,
) -> Result<ProvisionOutcome, ProvisionError<S::Error>> {
    storage
        .write_byte(COMMIT_MARKER_OFFSET, marker)
        .map_err(|source| ProvisionError::CommitUnverified(CommitError::Write(source)))?;

    let committed = read_record(storage)
        .map_err(|source| ProvisionError::CommitUnverified(CommitError::Read(source)))?;

    match decode_record(&committed) == Ok(token) {
        true => Ok(ProvisionOutcome::Provisioned),
        false => Err(ProvisionError::CommitUnverified(CommitError::Mismatch)),
    }
}

fn read_record<S: IdentityStorage>(storage: &mut S) -> Result<[u8; EEPROM_RECORD_LEN], S::Error> {
    let mut record = [0; EEPROM_RECORD_LEN];

    for (offset, byte) in record.iter_mut().enumerate() {
        *byte = storage.read_byte(offset)?;
    }

    Ok(record)
}
