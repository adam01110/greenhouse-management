use super::{FixedClock, REG, sql, sync};
use crate::{Clock, ClockError, IngestError, Ingestor, ReceivedAtMs, Store, StoreError};
use rusqlite::Connection;

struct FailingClock;
impl Clock for FailingClock {
    fn received_at(&mut self) -> Result<ReceivedAtMs, ClockError> {
        Err(StoreError::InvalidTime.into())
    }
}
#[test]
fn clock_and_sql_errors_invalidate_source() {
    let dir = tempfile::tempdir().expect("directory");
    let path = dir.path().join("db");
    let store = Store::open(&path).expect("store");
    let mut ingest = Ingestor::new(store, FailingClock, true);
    let id = ingest.open_session().expect("open");
    sync(&mut ingest, id);
    assert!(matches!(
        ingest.feed(id, REG, &mut Vec::new(), &mut |_| {}),
        Err(IngestError::Clock(_))
    ));
    assert!(matches!(
        ingest.feed(id, REG, &mut Vec::new(), &mut |_| {}),
        Err(IngestError::InvalidSession)
    ));
    drop(ingest);
    let store = Store::open(&path).expect("reopen");
    let mut ingest = Ingestor::new(store, FixedClock, true);
    let a = ingest.open_session().expect("open");
    sync(&mut ingest, a);
    ingest
        .feed(a, REG, &mut Vec::new(), &mut |_| {})
        .expect("register");
    let conn = Connection::open(&path).expect("inspect");
    conn.execute_batch("CREATE TRIGGER fail_touch BEFORE UPDATE ON device BEGIN SELECT RAISE(ABORT,'failure'); END").expect("trigger");
    assert!(matches!(
        ingest.feed(a, b"HB,0,1\r\n", &mut Vec::new(), &mut |_| {}),
        Err(IngestError::Store(StoreError::Sql(_)))
    ));
    assert_eq!(sql(&path, "SELECT count(*) FROM heartbeat"), 0);
    conn.execute_batch("DROP TRIGGER fail_touch")
        .expect("drop trigger");
    let b = ingest.open_session().expect("reopen session");
    sync(&mut ingest, b);
    ingest
        .feed(b, REG, &mut Vec::new(), &mut |_| {})
        .expect("lease released");
    ingest
        .feed(b, b"HB,0,1\r\n", &mut Vec::new(), &mut |_| {})
        .expect("recovery");
    assert_eq!(sql(&path, "SELECT count(*) FROM heartbeat"), 1);
}
