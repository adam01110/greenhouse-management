use crate::StoreError;
use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior};

// SQLite application ID is the ASCII identifier "GRHE".
const APPLICATION_ID: i64 = 0x4752_4845;
const VERSION: i64 = 1;
const SCHEMA: [(&str, &str); 5] = [
    (
        "device",
        r"CREATE TABLE device (
    id INTEGER PRIMARY KEY CHECK(id BETWEEN 0 AND 255),
    token BLOB NOT NULL UNIQUE CHECK(length(token) = 16),
    name TEXT,
    first_seen_ms INTEGER NOT NULL CHECK(first_seen_ms >= 0),
    last_seen_ms INTEGER NOT NULL CHECK(last_seen_ms >= first_seen_ms)
) STRICT",
    ),
    (
        "reading",
        r"CREATE TABLE reading (
    id INTEGER PRIMARY KEY,
    device_id INTEGER NOT NULL REFERENCES device(id),
    received_at_ms INTEGER NOT NULL CHECK(received_at_ms >= 0),
    device_uptime_ms INTEGER NOT NULL CHECK(device_uptime_ms BETWEEN 0 AND 4294967295),
    moisture_raw INTEGER NOT NULL CHECK(moisture_raw BETWEEN 0 AND 1023),
    pump_state INTEGER NOT NULL CHECK(pump_state IN (0,
    1))
) STRICT",
    ),
    (
        "heartbeat",
        r"CREATE TABLE heartbeat (
    id INTEGER PRIMARY KEY,
    device_id INTEGER NOT NULL REFERENCES device(id),
    received_at_ms INTEGER NOT NULL CHECK(received_at_ms >= 0),
    device_uptime_ms INTEGER NOT NULL CHECK(device_uptime_ms BETWEEN 0 AND 4294967295)
) STRICT",
    ),
    (
        "reading_device_time",
        r"CREATE INDEX reading_device_time ON reading(device_id, received_at_ms, id)",
    ),
    (
        "heartbeat_device_time",
        r"CREATE INDEX heartbeat_device_time ON heartbeat(device_id, received_at_ms, id)",
    ),
];

#[derive(Eq, PartialEq)]
struct Metadata {
    application: i64,
    version: i64,
    objects: i64,
}

pub(crate) fn initialize(connection: &mut Connection) -> Result<(), StoreError> {
    let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;

    initialize_schema(&tx)?;
    tx.commit()?;

    Ok(())
}

fn metadata(tx: &Transaction<'_>) -> Result<Metadata, StoreError> {
    Ok(Metadata {
        application: tx.pragma_query_value(None, "application_id", |row| row.get(0))?,
        version: tx.pragma_query_value(None, "user_version", |row| row.get(0))?,
        objects: tx.query_row(
            "SELECT count(*) FROM sqlite_schema WHERE name NOT LIKE 'sqlite_%'",
            [],
            |row| row.get(0),
        )?,
    })
}

fn initialize_schema(tx: &Transaction<'_>) -> Result<(), StoreError> {
    let current = metadata(tx)?;
    match current
        == (Metadata {
            application: 0,
            version: 0,
            objects: 0,
        }) {
        true => create_schema(tx),
        false => verify_schema(tx, &current),
    }
}

fn verify_schema(tx: &Transaction<'_>, current: &Metadata) -> Result<(), StoreError> {
    let expected = Metadata {
        application: APPLICATION_ID,
        version: VERSION,
        objects: i64::try_from(SCHEMA.len()).map_err(|_size| StoreError::Schema)?,
    };

    if current == &expected {
        for (name, sql) in SCHEMA {
            verify_object(tx, name, sql)?;
        }

        Ok(())
    } else {
        Err(StoreError::Schema)
    }
}

fn verify_object(tx: &Transaction<'_>, name: &str, expected: &str) -> Result<(), StoreError> {
    let actual: Option<String> = tx
        .prepare_cached("SELECT sql FROM sqlite_schema WHERE name = ?1")?
        .query_row([name], |row| row.get(0))
        .optional()?;

    if actual.as_deref() == Some(expected) {
        Ok(())
    } else {
        Err(StoreError::Schema)
    }
}

fn create_schema(tx: &Transaction<'_>) -> Result<(), StoreError> {
    for (_, sql) in SCHEMA {
        tx.execute_batch(sql)?;
    }

    tx.pragma_update(None, "application_id", APPLICATION_ID)?;
    tx.pragma_update(None, "user_version", VERSION)?;

    Ok(())
}

pub(crate) fn configure(connection: &Connection) -> Result<(), StoreError> {
    connection.pragma_update(None, "foreign_keys", true)?;
    ensure_wal(connection)?;
    connection.pragma_update(None, "synchronous", "FULL")?;

    let _: String = connection.query_row("SELECT json_object('ok', 1)", [], |row| row.get(0))?;

    Ok(())
}

fn ensure_wal(connection: &Connection) -> Result<(), StoreError> {
    let mode: String = connection.pragma_query_value(None, "journal_mode", |row| row.get(0))?;
    if mode.eq_ignore_ascii_case("wal") {
        return Ok(());
    }
    let mode: String = connection.query_row("PRAGMA journal_mode=WAL", [], |row| row.get(0))?;
    if !mode.eq_ignore_ascii_case("wal") {
        return Err(StoreError::WalUnavailable);
    }
    Ok(())
}
