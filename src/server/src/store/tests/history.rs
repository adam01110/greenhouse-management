use super::{database, scalar, time, token};
use crate::{ReceivedAtMs, StoreError};
use greenhouse_protocol::{Message, MoistureRaw};
use rusqlite::ErrorCode;

#[test]
fn telemetry_history_and_clock_rollback() {
    let (_dir, mut store) = database();
    let id = store
        .register(token(0), time(100), true)
        .expect("provision device");
    store
        .record(
            Message::Data {
                device_id: id,
                uptime_ms: u32::MAX,
                moisture_raw: MoistureRaw::new(1023).expect("valid ADC maximum"),
                pump_on: true,
            },
            time(200),
        )
        .expect("store DATA");

    for uptime_ms in [u32::MAX, 0, 0] {
        store
            .record(
                Message::Heartbeat {
                    device_id: id,
                    uptime_ms,
                },
                time(150),
            )
            .expect("store every heartbeat");
    }

    let data: (i64, i64, i64, i64) = store
        .connection
        .query_row(
            "SELECT received_at_ms, device_uptime_ms, moisture_raw, pump_state FROM reading",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .expect("read stored DATA fields");
    assert_eq!(data, (200, 4_294_967_295, 1023, 1));

    let mut query = store
        .connection
        .prepare("SELECT received_at_ms, device_uptime_ms FROM heartbeat ORDER BY id")
        .expect("prepare heartbeat inspection");
    let values: Vec<(i64, i64)> = query
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .expect("query heartbeats")
        .collect::<Result<_, _>>()
        .expect("read all heartbeat rows");
    drop(query);
    assert_eq!(values, vec![(150, 4_294_967_295), (150, 0), (150, 0)]);
    assert_eq!(scalar(&store, "SELECT last_seen_ms FROM device"), 200);

    assert!(matches!(
        store.record(
            Message::Heartbeat {
                device_id: 200,
                uptime_ms: 0
            },
            time(300)
        ),
        Err(StoreError::UnknownDevice(200))
    ));
    assert!(matches!(
        ReceivedAtMs::new(-1),
        Err(StoreError::InvalidTime)
    ));
    assert!(matches!(
        ReceivedAtMs::new(i128::MAX),
        Err(StoreError::InvalidTime)
    ));
}

#[test]
fn failed_touch_rolls_back_insert_then_recovers() {
    let (_dir, mut store) = database();
    let id = store
        .register(token(0), time(1), true)
        .expect("provision device");
    store
        .connection
        .execute_batch(
            "CREATE TRIGGER fail_touch BEFORE UPDATE ON device
         BEGIN SELECT RAISE(ABORT, 'failure'); END",
        )
        .expect("install failing last-seen trigger");

    let heartbeat = Message::Heartbeat {
        device_id: id,
        uptime_ms: 1,
    };
    assert!(matches!(
        store.record(heartbeat, time(2)),
        Err(StoreError::Sql(_))
    ));
    assert_eq!(scalar(&store, "SELECT count(*) FROM heartbeat"), 0);
    assert_eq!(scalar(&store, "SELECT last_seen_ms FROM device"), 1);

    store
        .connection
        .execute_batch("DROP TRIGGER fail_touch")
        .expect("remove failure injection");
    store
        .record(heartbeat, time(2))
        .expect("recover after failed transaction");
    assert_eq!(scalar(&store, "SELECT count(*) FROM heartbeat"), 1);
}

#[test]
fn deferred_commit_failure_rolls_back_history_and_last_seen() {
    let (_dir, mut store) = database();
    let id = store
        .register(token(0), time(1), true)
        .expect("provision device");
    store
        .connection
        .execute_batch(
            "CREATE TABLE commit_fault (
            device_id INTEGER REFERENCES device(id) DEFERRABLE INITIALLY DEFERRED
         ) STRICT;
         CREATE TRIGGER fail_commit AFTER INSERT ON heartbeat
         BEGIN INSERT INTO commit_fault VALUES (255); END;",
        )
        .expect("install deferred constraint failure");

    let heartbeat = Message::Heartbeat {
        device_id: id,
        uptime_ms: 10,
    };
    let error = store
        .record(heartbeat, time(2))
        .expect_err("COMMIT must reject deferred foreign key");
    assert!(
        matches!(error, StoreError::Sql(ref source) if source.sqlite_error_code() == Some(ErrorCode::ConstraintViolation))
    );
    assert!(
        store.connection.is_autocommit(),
        "failed COMMIT must end its transaction"
    );
    assert_eq!(scalar(&store, "SELECT count(*) FROM heartbeat"), 0);
    assert_eq!(scalar(&store, "SELECT count(*) FROM commit_fault"), 0);
    assert_eq!(scalar(&store, "SELECT last_seen_ms FROM device"), 1);

    store
        .connection
        .execute_batch("DROP TRIGGER fail_commit; DROP TABLE commit_fault;")
        .expect("remove deferred fault");
    store
        .record(heartbeat, time(3))
        .expect("recover after failed COMMIT");
    assert_eq!(scalar(&store, "SELECT count(*) FROM heartbeat"), 1);
    assert_eq!(scalar(&store, "SELECT last_seen_ms FROM device"), 3);
}
