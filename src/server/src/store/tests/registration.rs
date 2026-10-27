use super::{database, scalar, time, token};
use crate::{Store, StoreError};
use std::{
    sync::{Arc, Barrier},
    thread,
};

#[test]
fn registration_capacity_and_reopen() {
    let (dir, mut store) = database();
    assert!(matches!(
        store.register(token(0), time(1), false),
        Err(StoreError::UnknownToken)
    ));

    for id in 0..=255_u16 {
        let assigned = store
            .register(token(id), time(1), true)
            .expect("allocate next ID");
        assert_eq!(u16::from(assigned), id);
    }
    assert!(matches!(
        store.register(token(256), time(2), true),
        Err(StoreError::ExhaustedIds)
    ));
    assert_eq!(
        store
            .register(token(23), time(2), false)
            .expect("lookup known token"),
        23
    );

    drop(store);
    let mut store = Store::open(&dir.path().join("test.sqlite")).expect("reopen managed database");
    assert_eq!(
        store
            .register(token(23), time(3), false)
            .expect("lookup persisted token"),
        23
    );
}

#[test]
fn concurrent_registration_is_serialized() {
    let (dir, store) = database();
    drop(store);
    let path = dir.path().join("test.sqlite");
    let barrier = Arc::new(Barrier::new(4));
    let mut handles = Vec::new();

    for value in [10, 10, 11, 12] {
        let path = path.clone();
        let barrier = Arc::clone(&barrier);
        handles.push(thread::spawn(move || {
            barrier.wait();
            let mut store = Store::open(&path).expect("open concurrent connection");
            store
                .register(token(value), time(1), true)
                .expect("register concurrently")
        }));
    }

    let ids: Vec<u8> = handles
        .into_iter()
        .map(|handle| handle.join().expect("registration thread succeeded"))
        .collect();
    assert_eq!(ids.first(), ids.get(1));
    let mut unique = ids;
    unique.sort_unstable();
    unique.dedup();
    assert_eq!(unique.len(), 3);
}

#[test]
fn failed_registration_does_not_consume_an_id() {
    let (_dir, mut store) = database();
    store
        .connection
        .execute_batch(
            "CREATE TRIGGER fail_registration AFTER INSERT ON device
         BEGIN SELECT RAISE(ABORT, 'registration failure'); END;",
        )
        .expect("install registration failure");
    assert!(matches!(
        store.register(token(7), time(1), true),
        Err(StoreError::Sql(_))
    ));
    assert_eq!(scalar(&store, "SELECT count(*) FROM device"), 0);
    store
        .connection
        .execute_batch("DROP TRIGGER fail_registration")
        .expect("remove registration fault");
    assert_eq!(
        store
            .register(token(7), time(2), true)
            .expect("retry registration"),
        0
    );
}

#[test]
fn known_registration_updates_are_transactional_and_clock_safe() {
    let (_dir, mut store) = database();
    let id = store
        .register(token(0), time(100), true)
        .expect("provision device");
    assert_eq!(
        store
            .register(token(0), time(50), false)
            .expect("repeat after clock rollback"),
        id
    );
    assert_eq!(scalar(&store, "SELECT last_seen_ms FROM device"), 100);
    store
        .connection
        .execute_batch(
            "CREATE TRIGGER fail_touch BEFORE UPDATE ON device
         BEGIN SELECT RAISE(ABORT, 'registration touch failure'); END;",
        )
        .expect("install registration update fault");
    assert!(matches!(
        store.register(token(0), time(200), false),
        Err(StoreError::Sql(_))
    ));
    assert_eq!(scalar(&store, "SELECT last_seen_ms FROM device"), 100);
    assert_eq!(scalar(&store, "SELECT count(*) FROM device"), 1);
}
