use super::{execute_cached, touch};
use crate::{ReceivedAtMs, StoreError};
use greenhouse_protocol::Message;
use rusqlite::{Transaction, params};

pub(crate) fn insert_message(
    tx: &Transaction<'_>,
    message: Message,
    received: ReceivedAtMs,
) -> Result<(), StoreError> {
    let (Message::Data { device_id, .. } | Message::Heartbeat { device_id, .. }) = message;
    ensure_device(tx, device_id)?;
    insert_history(tx, message, received)?;
    touch(tx, device_id, received)
}

fn ensure_device(tx: &Transaction<'_>, id: u8) -> Result<(), StoreError> {
    let exists: bool = tx
        .prepare_cached("SELECT EXISTS(SELECT 1 FROM device WHERE id = ?1)")?
        .query_row([id], |row| row.get(0))?;
    if !exists {
        return Err(StoreError::UnknownDevice(id));
    }
    Ok(())
}

fn insert_history(
    tx: &Transaction<'_>,
    message: Message,
    received: ReceivedAtMs,
) -> Result<(), StoreError> {
    match message {
        Message::Data { device_id, uptime_ms, moisture_raw, pump_on } => execute_cached(tx,
            "INSERT INTO reading(device_id, received_at_ms, device_uptime_ms, moisture_raw, pump_state)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![device_id, received.0, uptime_ms, moisture_raw.get(), pump_on]),
        Message::Heartbeat { device_id, uptime_ms } => execute_cached(tx,
            "INSERT INTO heartbeat(device_id, received_at_ms, device_uptime_ms) VALUES (?1, ?2, ?3)",
            params![device_id, received.0, uptime_ms]),
    }
}
