//! Registration, framed UART ingestion and persistent telemetry storage.

use greenhouse_protocol::{DeviceToken, Message};
use rusqlite::{Connection, TransactionBehavior};
use std::{path::Path, time::Duration};
use thiserror::Error;

mod ingest;
mod store;

pub use ingest::{Clock, ClockError, IngestError, Ingestor, Rejection, SessionId, SystemClock};

/// Server reception time in Unix milliseconds, never device uptime.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ReceivedAtMs(i64);

impl ReceivedAtMs {
    /// Checks a Unix millisecond value before it reaches SQLite.
    ///
    /// # Errors
    /// Returns `InvalidTime` for negative values or values exceeding `i64::MAX`.
    pub fn new(value: i128) -> Result<Self, StoreError> {
        let value = i64::try_from(value).map_err(|_error| StoreError::InvalidTime)?;

        if value < 0 {
            Err(StoreError::InvalidTime)
        } else {
            Ok(Self(value))
        }
    }
}

#[derive(Debug, Error)]
pub enum StoreError {
    #[error("SQLite error: {0}")]
    Sql(#[from] rusqlite::Error),
    #[error("invalid Unix millisecond timestamp")]
    InvalidTime,
    #[error("unrecognized or incompatible database schema")]
    Schema,
    #[error("unknown registration token")]
    UnknownToken,
    #[error("unknown device ID {0}")]
    UnknownDevice(u8),
    #[error("all device IDs are allocated")]
    ExhaustedIds,
    #[error("SQLite WAL mode is unavailable")]
    WalUnavailable,
}

/// One reusable SQLite connection. Calls are synchronous and may block.
pub struct Store {
    connection: Connection,
}

impl Store {
    /// Opens or initializes a private managed database.
    ///
    /// # Errors
    /// Returns an error for an incompatible database or a SQLite failure.
    pub fn open(path: &Path) -> Result<Self, StoreError> {
        let mut connection = Connection::open(path)?;
        connection.busy_timeout(Duration::from_secs(5))?;

        store::initialize(&mut connection)?;
        store::configure(&connection)?;

        Ok(Self { connection })
    }

    /// Registers a token or retrieves its existing ID.
    ///
    /// # Errors
    /// Returns an error if provisioning is disabled, IDs are exhausted, or SQLite fails.
    pub fn register(
        &mut self,
        token: DeviceToken,
        received: ReceivedAtMs,
        allow_provision: bool,
    ) -> Result<u8, StoreError> {
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;

        let id = store::register_token(&tx, token, received, allow_provision)?;
        tx.commit()?;

        Ok(id)
    }

    /// Stores a DATA or HB message atomically with its last-seen update.
    ///
    /// # Errors
    /// Returns an error for an unknown device or a SQLite failure.
    pub fn record(&mut self, message: Message, received: ReceivedAtMs) -> Result<(), StoreError> {
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;

        store::insert_message(&tx, message, received)?;
        tx.commit()?;

        Ok(())
    }
}
