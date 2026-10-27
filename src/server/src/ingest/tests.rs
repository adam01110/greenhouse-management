use super::{Clock, ClockError, Ingestor, ReceivedAtMs, SessionId, Store};
use rusqlite::Connection;
use std::path::Path;
use tempfile::TempDir;

mod failures;
mod framing;
mod replies;
mod sessions;

struct FixedClock;
impl Clock for FixedClock {
    fn received_at(&mut self) -> Result<ReceivedAtMs, ClockError> {
        Ok(ReceivedAtMs::new(100)?)
    }
}
fn setup(provision: bool) -> (TempDir, Ingestor<FixedClock>) {
    let dir = tempfile::tempdir().expect("temporary directory");
    let store = Store::open(&dir.path().join("db")).expect("open store");
    (dir, Ingestor::new(store, FixedClock, provision))
}
fn sql(path: &Path, query: &str) -> i64 {
    Connection::open(path)
        .expect("inspection connection")
        .query_row(query, [], |r| r.get(0))
        .expect("inspection query")
}
const REG: &[u8] = b"REG,00000000000000000000000000000000\r\n";
fn sync<C: Clock>(ingest: &mut Ingestor<C>, id: SessionId) {
    assert_eq!(feed_ok(ingest, id, b"\n"), []);
}

fn feed_ok<C: Clock>(ingest: &mut Ingestor<C>, id: SessionId, input: &[u8]) -> Vec<u8> {
    let mut reply = Vec::new();
    let mut rejected = Vec::new();
    ingest
        .feed(id, input, &mut reply, &mut |reason| rejected.push(reason))
        .expect("ingest valid test input");
    assert_eq!(rejected, [], "valid test input must not be rejected");
    reply
}
