use crate::{ReceivedAtMs, StoreError};
use rusqlite::{Connection, Params, Transaction, params};

mod history;
mod registration;
mod schema;
#[cfg(test)]
mod tests;

pub(super) use history::insert_message;
pub(super) use registration::register_token;
pub(super) use schema::{configure, initialize};

fn execute_cached(
    connection: &Connection,
    sql: &str,
    values: impl Params,
) -> Result<(), StoreError> {
    connection.prepare_cached(sql)?.execute(values)?;
    Ok(())
}

fn touch(tx: &Transaction<'_>, id: u8, received: ReceivedAtMs) -> Result<(), StoreError> {
    execute_cached(
        tx,
        "UPDATE device SET last_seen_ms = max(last_seen_ms, ?1) WHERE id = ?2",
        params![received.0, id],
    )
}
