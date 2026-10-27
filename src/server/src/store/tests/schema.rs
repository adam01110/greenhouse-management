use super::{assert_constraint, database, scalar, time, token};
use crate::{Store, StoreError};
use rusqlite::Connection;

#[test]
fn schema_configuration_constraints_and_indices() {
    let (_dir, store) = database();

    assert_eq!(scalar(&store, "PRAGMA foreign_keys"), 1);
    assert_eq!(scalar(&store, "PRAGMA synchronous"), 2);

    let journal: String = store
        .connection
        .pragma_query_value(None, "journal_mode", |row| row.get(0))
        .expect("read journal mode");

    assert_eq!(journal, "wal");

    assert_constraint(store.connection.execute(
        "INSERT INTO heartbeat (device_id, received_at_ms, device_uptime_ms)
         VALUES (9, 1, 1)",
        [],
    ));
    assert_constraint(store.connection.execute(
        "INSERT INTO device (id, token, first_seen_ms, last_seen_ms)
         VALUES (1, zeroblob(16), 'bad', 1)",
        [],
    ));

    for table in ["reading", "heartbeat"] {
        let sql = format!(
            "EXPLAIN QUERY PLAN SELECT * FROM {table} WHERE device_id = ?1 ORDER BY received_at_ms, id"
        );
        let plan: String = store
            .connection
            .query_row(&sql, [0], |row| row.get(3))
            .expect("inspect history query plan");

        assert!(plan.contains(&format!("{table}_device_time")));
    }
}

#[test]
fn foreign_database_is_not_modified() {
    let dir = tempfile::tempdir().expect("create test directory");
    let path = dir.path().join("foreign.sqlite");
    let connection = Connection::open(&path).expect("create foreign database");

    connection
        .execute_batch("CREATE TABLE old_data (value TEXT); INSERT INTO old_data VALUES ('keep')")
        .expect("write foreign data");
    drop(connection);

    assert!(matches!(Store::open(&path), Err(StoreError::Schema)));

    let connection = Connection::open(&path).expect("reopen foreign database");
    let value: String = connection
        .query_row("SELECT value FROM old_data", [], |row| row.get(0))
        .expect("read preserved value");

    assert_eq!(value, "keep");

    let journal: String = connection
        .pragma_query_value(None, "journal_mode", |row| row.get(0))
        .expect("read unchanged journal mode");

    assert_eq!(journal, "delete");
}

#[test]
fn incompatible_managed_schema_is_rejected_without_changes() {
    let (dir, store) = database();
    store
        .connection
        .execute_batch(
            "DROP INDEX reading_device_time;
             CREATE INDEX reading_device_time ON reading(received_at_ms)",
        )
        .expect("make schema incompatible");
    drop(store);
    let path = dir.path().join("test.sqlite");

    assert!(matches!(Store::open(&path), Err(StoreError::Schema)));

    let connection = Connection::open(&path).expect("inspect incompatible database");
    let sql: String = connection
        .query_row(
            "SELECT sql FROM sqlite_schema WHERE name = 'reading_device_time'",
            [],
            |row| row.get(0),
        )
        .expect("read preserved index definition");

    assert_eq!(
        sql,
        "CREATE INDEX reading_device_time ON reading(received_at_ms)"
    );
}

#[test]
fn database_enforces_all_telemetry_bounds() {
    let (_dir, mut store) = database();
    assert_eq!(
        store
            .register(token(0), time(1), true)
            .expect("provision device"),
        0
    );

    let valid_token = [1_u8; 16];
    let short_token = [0_u8; 15];
    let zero_token = [0_u8; 16];

    let device_cases: [(&str, i64, &[u8], i64, i64); 5] = [
        ("device ID above 255", 256, &valid_token, 1, 1),
        ("short token", 1, &short_token, 1, 1),
        ("all-zero token", 1, &zero_token, 1, 1),
        ("negative first-seen time", 1, &valid_token, -1, 1),
        ("last-seen before first-seen", 1, &valid_token, 2, 1),
    ];

    for (name, id, token, first_seen, last_seen) in device_cases {
        let result = store.connection.execute(
            "INSERT INTO device (id, token, name, first_seen_ms, last_seen_ms)
             VALUES (?1, ?2, NULL, ?3, ?4)",
            rusqlite::params![id, token, first_seen, last_seen],
        );

        assert!(result.is_err(), "case unexpectedly succeeded: {name}");
        assert_constraint(result);
    }

    let reading_cases = [
        ("negative receipt time", -1_i64, 0_i64, 0, 0),
        ("uptime above u32 maximum", 1, 4_294_967_296, 0, 0),
        ("moisture above 1023", 1, 0, 1024, 0),
        ("pump state above 1", 1, 0, 0, 2),
    ];

    for (name, received, uptime, moisture, pump) in reading_cases {
        let result = store.connection.execute(
            "INSERT INTO reading (device_id, received_at_ms, device_uptime_ms, moisture_raw, pump_state)
             VALUES (0, ?1, ?2, ?3, ?4)",
            rusqlite::params![received, uptime, moisture, pump],
        );

        assert!(result.is_err(), "case unexpectedly succeeded: {name}");
        assert_constraint(result);
    }

    let heartbeat_cases = [("negative uptime", 1, -1), ("negative receipt time", -1, 0)];
    for (name, received, uptime) in heartbeat_cases {
        let result = store.connection.execute(
            "INSERT INTO heartbeat (device_id, received_at_ms, device_uptime_ms)
             VALUES (0, ?1, ?2)",
            rusqlite::params![received, uptime],
        );

        assert!(result.is_err(), "case unexpectedly succeeded: {name}");
        assert_constraint(result);
    }

    assert_eq!(scalar(&store, "SELECT count(*) FROM device"), 1);
    assert_eq!(scalar(&store, "SELECT count(*) FROM reading"), 0);
    assert_eq!(scalar(&store, "SELECT count(*) FROM heartbeat"), 0);
}
