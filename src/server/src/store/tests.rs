use crate::{ReceivedAtMs, Store};
use greenhouse_protocol::DeviceToken;
use rusqlite::ErrorCode;
use tempfile::TempDir;

mod history;
mod registration;
mod schema;

fn time(value: i128) -> ReceivedAtMs {
    ReceivedAtMs::new(value).expect("valid test timestamp")
}

fn token(value: u16) -> DeviceToken {
    let [high, low] = value.to_be_bytes();
    let mut bytes = [0; 16];
    bytes[0] = high;
    bytes[1] = low;
    DeviceToken::new(bytes)
}

fn database() -> (TempDir, Store) {
    let dir = tempfile::tempdir().expect("create isolated database directory");
    let store = Store::open(&dir.path().join("test.sqlite")).expect("initialize test database");
    (dir, store)
}

fn scalar(store: &Store, sql: &str) -> i64 {
    store
        .connection
        .query_row(sql, [], |row| row.get(0))
        .expect("read database scalar")
}

fn assert_constraint(result: Result<usize, rusqlite::Error>) {
    let error = result.expect_err("invalid row must fail its constraint");
    assert_eq!(
        error.sqlite_error_code(),
        Some(ErrorCode::ConstraintViolation)
    );
}
