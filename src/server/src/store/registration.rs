use super::{execute_cached, touch};
use crate::{ReceivedAtMs, StoreError};
use greenhouse_protocol::DeviceToken;
use rusqlite::{OptionalExtension, Transaction, params};

pub(crate) fn register_token(
    tx: &Transaction<'_>,
    token: DeviceToken,
    received: ReceivedAtMs,
    allow_provision: bool,
) -> Result<u8, StoreError> {
    if let Some(id) = lookup(tx, token)? {
        touch(tx, id, received)?;
        return Ok(id);
    }
    if !allow_provision {
        return Err(StoreError::UnknownToken);
    }
    allocate(tx, token, received)
}

fn lookup(tx: &Transaction<'_>, token: DeviceToken) -> Result<Option<u8>, StoreError> {
    Ok(tx
        .prepare_cached("SELECT id FROM device WHERE token = ?1")?
        .query_row([token.as_bytes().as_slice()], |row| row.get(0))
        .optional()?)
}

fn allocate(
    tx: &Transaction<'_>,
    token: DeviceToken,
    received: ReceivedAtMs,
) -> Result<u8, StoreError> {
    let id = next_available_id(tx)?.ok_or(StoreError::ExhaustedIds)?;
    execute_cached(
        tx,
        "INSERT INTO device(id, token, first_seen_ms, last_seen_ms) VALUES (?1, ?2, ?3, ?3)",
        params![id, token.as_bytes().as_slice(), received.0],
    )?;
    Ok(id)
}

fn next_available_id(tx: &Transaction<'_>) -> Result<Option<u8>, StoreError> {
    Ok(tx
        .prepare_cached(
            "WITH RECURSIVE ids(id) AS (
            VALUES(0) UNION ALL SELECT id + 1 FROM ids WHERE id < 255
         )
         SELECT id FROM ids
         WHERE NOT EXISTS (SELECT 1 FROM device WHERE device.id = ids.id)
         ORDER BY id LIMIT 1",
        )?
        .query_row([], |row| row.get(0))
        .optional()?)
}
