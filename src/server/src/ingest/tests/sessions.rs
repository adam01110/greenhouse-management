use super::{FixedClock, REG, feed_ok, setup, sql, sync};
use crate::{Clock, ClockError, IngestError, Ingestor, ReceivedAtMs, Rejection, Store, StoreError};
use greenhouse_protocol::{DeviceToken, HostFrame};

#[test]
fn source_lease_mismatch_and_changed_token() {
    let (dir, mut ingest) = setup(true);
    let a = ingest.open_session().expect("a");
    let b = ingest.open_session().expect("b");
    sync(&mut ingest, a);
    sync(&mut ingest, b);
    let mut output = Vec::new();
    let mut rejects = Vec::new();
    ingest
        .feed(b, b"HB,0,0\r\n", &mut output, &mut |r| rejects.push(r))
        .expect("unregistered");
    ingest
        .feed(a, REG, &mut output, &mut |r| rejects.push(r))
        .expect("register");
    ingest
        .feed(b, REG, &mut output, &mut |r| rejects.push(r))
        .expect("duplicate");
    ingest
        .feed(a, b"HB,1,0\r\n", &mut output, &mut |r| rejects.push(r))
        .expect("wrong id");
    assert_eq!(
        rejects,
        vec![
            Rejection::Unregistered,
            Rejection::DuplicateToken,
            Rejection::WrongId
        ]
    );
    assert_eq!(
        sql(&dir.path().join("db"), "SELECT count(*) FROM heartbeat"),
        0
    );
    let changed = b"REG,00000000000000000000000000000001\r\n";
    assert!(matches!(
        ingest.feed(a, changed, &mut output, &mut |_| {}),
        Err(IngestError::TokenChanged)
    ));
    assert!(matches!(
        ingest.feed(a, REG, &mut output, &mut |_| {}),
        Err(IngestError::InvalidSession)
    ));
    ingest
        .feed(b, REG, &mut output, &mut |_| {})
        .expect("lease released");
    ingest.close_session(b);
    ingest.close_session(b);
}

#[test]
fn disabled_provision_is_fatal_and_partial_frames_do_not_cross_sessions() {
    let (_dir, mut ingest) = setup(false);
    let a = ingest.open_session().expect("a");
    sync(&mut ingest, a);
    ingest
        .feed(a, b"REG,00", &mut Vec::new(), &mut |_| {})
        .expect("partial");
    ingest.close_session(a);
    let b = ingest.open_session().expect("b");
    sync(&mut ingest, b);
    assert!(matches!(
        ingest.feed(b, REG, &mut Vec::new(), &mut |_| {}),
        Err(IngestError::Store(StoreError::UnknownToken))
    ));
    assert!(matches!(
        ingest.feed(b, REG, &mut Vec::new(), &mut |_| {}),
        Err(IngestError::InvalidSession)
    ));
}

#[test]
fn last_id_and_exhaustion() {
    let dir = tempfile::tempdir().expect("directory");
    let path = dir.path().join("db");
    let mut store = Store::open(&path).expect("store");
    for value in 0..255_u16 {
        let mut token = [0; 16];
        let [a, b] = value.to_be_bytes();
        token[0] = a;
        token[1] = b;
        assert_eq!(
            u16::from(
                store
                    .register(
                        DeviceToken::new(token),
                        ReceivedAtMs::new(1).expect("time"),
                        true
                    )
                    .expect("provision")
            ),
            value
        );
    }
    let mut ingest = Ingestor::new(store, FixedClock, true);
    let id = ingest.open_session().expect("session");
    sync(&mut ingest, id);
    let mut out = Vec::new();
    let last = b"REG,00ff0000000000000000000000000000\r\n";
    ingest
        .feed(id, last, &mut out, &mut |_| {})
        .expect("last id");
    assert!(matches!(
        greenhouse_protocol::parse_host_line(&out),
        Ok(HostFrame::Assigned { device_id: 255, .. })
    ));
    let next = ingest.open_session().expect("session");
    sync(&mut ingest, next);
    assert!(matches!(
        ingest.feed(
            next,
            b"REG,01000000000000000000000000000000\r\n",
            &mut out,
            &mut |_| {}
        ),
        Err(IngestError::Store(StoreError::ExhaustedIds))
    ));
}

#[test]
fn repeated_registration_preserves_the_live_binding() {
    let (dir, mut ingest) = setup(true);
    let id = ingest.open_session().expect("open source");
    sync(&mut ingest, id);
    let first = feed_ok(&mut ingest, id, REG);
    feed_ok(&mut ingest, id, b"HB,0,10\r\n");
    assert_eq!(feed_ok(&mut ingest, id, REG), first);
    feed_ok(&mut ingest, id, b"HB,0,11\r\n");
    assert_eq!(
        sql(&dir.path().join("db"), "SELECT count(*) FROM device"),
        1
    );
    assert_eq!(
        sql(&dir.path().join("db"), "SELECT count(*) FROM heartbeat"),
        2
    );

    let other = ingest.open_session().expect("open competing source");
    sync(&mut ingest, other);
    let mut output = Vec::new();
    let mut rejected = Vec::new();
    ingest
        .feed(other, REG, &mut output, &mut |reason| rejected.push(reason))
        .expect("reject duplicate without evicting original source");
    assert_eq!(rejected, [Rejection::DuplicateToken]);
    assert_eq!(output, []);
    feed_ok(&mut ingest, id, b"HB,0,12\r\n");
    assert_eq!(
        sql(&dir.path().join("db"), "SELECT count(*) FROM heartbeat"),
        3
    );
}

struct AdvancingClock(i128);
impl Clock for AdvancingClock {
    fn received_at(&mut self) -> Result<ReceivedAtMs, ClockError> {
        let value = ReceivedAtMs::new(self.0)?;
        self.0 += 1;
        Ok(value)
    }
}

#[test]
fn registered_sources_cannot_write_each_others_history() {
    let dir = tempfile::tempdir().expect("database directory");
    let path = dir.path().join("db");
    let store = Store::open(&path).expect("create store");
    let mut ingest = Ingestor::new(store, AdvancingClock(100), true);
    let a = ingest.open_session().expect("source a");
    let b = ingest.open_session().expect("source b");
    sync(&mut ingest, a);
    sync(&mut ingest, b);
    feed_ok(&mut ingest, a, REG);
    feed_ok(&mut ingest, b, b"REG,00000000000000000000000000000001\r\n");
    assert_eq!(ingest.clock.0, 102);

    let duplicate = ingest.open_session().expect("duplicate source");
    sync(&mut ingest, duplicate);
    let mut duplicate_reply = Vec::new();
    let mut rejected = Vec::new();
    ingest
        .feed(duplicate, REG, &mut duplicate_reply, &mut |r| {
            rejected.push(r);
        })
        .expect("reject duplicate before consulting clock or store");
    assert_eq!(rejected, [Rejection::DuplicateToken]);
    assert_eq!(duplicate_reply, []);
    assert_eq!(ingest.clock.0, 102);
    ingest.close_session(duplicate);
    rejected.clear();
    ingest
        .feed(a, b"HB,1,10\r\n", &mut Vec::new(), &mut |r| {
            rejected.push(r);
        })
        .expect("reject a's foreign ID");
    ingest
        .feed(b, b"DATA,0,20,123,0\r\n", &mut Vec::new(), &mut |r| {
            rejected.push(r);
        })
        .expect("reject b's foreign ID");
    assert_eq!(rejected, [Rejection::WrongId, Rejection::WrongId]);
    assert_eq!(
        ingest.clock.0, 102,
        "rejected frames must not consume reception timestamps"
    );
    assert_eq!(sql(&path, "SELECT count(*) FROM heartbeat"), 0);
    assert_eq!(sql(&path, "SELECT count(*) FROM reading"), 0);
    assert_eq!(
        sql(&path, "SELECT last_seen_ms FROM device WHERE id=0"),
        100
    );
    assert_eq!(
        sql(&path, "SELECT last_seen_ms FROM device WHERE id=1"),
        101
    );

    feed_ok(&mut ingest, a, b"HB,0,10\r\n");
    feed_ok(&mut ingest, b, b"DATA,1,20,123,0\r\n");
    assert_eq!(
        sql(
            &path,
            "SELECT count(*) FROM heartbeat WHERE device_id=0 AND received_at_ms=102"
        ),
        1
    );
    assert_eq!(
        sql(
            &path,
            "SELECT count(*) FROM reading WHERE device_id=1 AND received_at_ms=103"
        ),
        1
    );
    ingest.close_session(a);
    feed_ok(&mut ingest, b, b"HB,1,21\r\n");
    assert_eq!(
        sql(&path, "SELECT last_seen_ms FROM device WHERE id=1"),
        104
    );
    assert_eq!(sql(&path, "SELECT count(*) FROM device"), 2);
}

#[test]
fn session_handles_never_wrap_or_reactivate_closed_sources() {
    let (_dir, mut ingest) = setup(true);
    let first = ingest.open_session().expect("first source");
    sync(&mut ingest, first);
    ingest.next = Some(u64::MAX);
    let last = ingest.open_session().expect("last representable handle");
    assert_ne!(first, last);
    sync(&mut ingest, last);
    assert!(matches!(
        ingest.open_session(),
        Err(IngestError::SessionExhausted)
    ));
    feed_ok(&mut ingest, first, REG);
    ingest.close_session(last);
    ingest.close_session(last);
    assert!(matches!(
        ingest.feed(last, REG, &mut Vec::new(), &mut |_| {}),
        Err(IngestError::InvalidSession)
    ));
    assert!(matches!(
        ingest.open_session(),
        Err(IngestError::SessionExhausted)
    ));
    feed_ok(&mut ingest, first, b"HB,0,0\r\n");
}
