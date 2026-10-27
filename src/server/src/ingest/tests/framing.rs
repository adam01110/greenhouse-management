use super::{REG, feed_ok, setup, sql, sync};
use crate::Rejection;
use greenhouse_protocol::{DeviceToken, HostFrame};

#[test]
fn every_split_registers_and_ingests_history() {
    for split in 0..=REG.len() {
        let (dir, mut ingest) = setup(true);
        let id = ingest.open_session().expect("open");
        sync(&mut ingest, id);
        let mut reply = Vec::new();
        let mut rejected = Vec::new();
        ingest
            .feed(
                id,
                REG.get(..split).expect("prefix"),
                &mut reply,
                &mut |r| rejected.push(r),
            )
            .expect("prefix");
        ingest
            .feed(
                id,
                REG.get(split..).expect("suffix"),
                &mut reply,
                &mut |r| rejected.push(r),
            )
            .expect("suffix");
        assert_eq!(
            greenhouse_protocol::parse_host_line(&reply),
            Ok(HostFrame::Assigned {
                token: DeviceToken::new([0; 16]),
                device_id: 0
            })
        );
        ingest
            .feed(
                id,
                b"DATA,0,4294967295,1023,1\r\nHB,0,0\r\nHB,0,0\r\n",
                &mut reply,
                &mut |r| rejected.push(r),
            )
            .expect("telemetry");
        assert_eq!(rejected, []);
        assert_eq!(
            sql(
                &dir.path().join("db"),
                "SELECT count(*) FROM reading
                 WHERE device_uptime_ms = 4294967295 AND moisture_raw = 1023 AND pump_state = 1"
            ),
            1
        );
        assert_eq!(
            sql(
                &dir.path().join("db"),
                "SELECT count(*) FROM heartbeat WHERE device_uptime_ms=0"
            ),
            2
        );
    }
}

#[test]
fn sync_overflow_and_malformed_recover_in_batch() {
    let (dir, mut ingest) = setup(true);
    let id = ingest.open_session().expect("open");
    let mut reply = Vec::new();
    let mut rejected = Vec::new();
    let mut input = Vec::from(&b"REG,discarded\r\n"[..]);
    input.extend([b'x'; 39]);
    input.extend_from_slice(b"\nnope\r\n");
    input.extend_from_slice(REG);
    ingest
        .feed(id, &input, &mut reply, &mut |r| rejected.push(r))
        .expect("batch");
    assert_eq!(rejected, vec![Rejection::Oversize, Rejection::Malformed]);
    assert_eq!(
        sql(&dir.path().join("db"), "SELECT count(*) FROM device"),
        1
    );
    greenhouse_protocol::parse_host_line(&reply).expect("valid response");
}

#[test]
fn telemetry_survives_every_split_and_bytewise_delivery() {
    let input = b"DATA,0,4294967295,1023,1\r\nHB,0,4294967295\r\nHB,0,0\r\n";
    for split in 0..=input.len() {
        let (dir, mut ingest) = setup(true);
        let id = ingest.open_session().expect("open source");
        sync(&mut ingest, id);
        feed_ok(&mut ingest, id, REG);
        assert_eq!(
            feed_ok(&mut ingest, id, input.get(..split).expect("prefix")),
            []
        );
        assert_eq!(
            feed_ok(&mut ingest, id, input.get(split..).expect("suffix")),
            []
        );
        assert_eq!(
            sql(&dir.path().join("db"), "SELECT count(*) FROM reading"),
            1
        );
        assert_eq!(
            sql(&dir.path().join("db"), "SELECT count(*) FROM heartbeat"),
            2
        );
    }

    let (dir, mut ingest) = setup(true);
    let id = ingest.open_session().expect("open bytewise source");
    sync(&mut ingest, id);
    feed_ok(&mut ingest, id, REG);
    for byte in input {
        assert_eq!(feed_ok(&mut ingest, id, std::slice::from_ref(byte)), []);
    }
    assert_eq!(
        sql(&dir.path().join("db"), "SELECT count(*) FROM reading"),
        1
    );
    assert_eq!(
        sql(&dir.path().join("db"), "SELECT count(*) FROM heartbeat"),
        2
    );
}

#[test]
fn attach_discards_even_a_complete_registration_and_overflow_lf_rearms() {
    let (dir, mut ingest) = setup(true);
    let id = ingest.open_session().expect("attach unsynchronized source");
    assert_eq!(feed_ok(&mut ingest, id, REG), []);
    assert_eq!(
        sql(&dir.path().join("db"), "SELECT count(*) FROM device"),
        0
    );

    let mut input = vec![b'x'; greenhouse_protocol::DeviceFrame::MAX_LEN];
    input.push(b'\n');
    input.extend_from_slice(REG);
    let mut output = Vec::new();
    let mut rejected = Vec::new();
    ingest
        .feed(id, &input, &mut output, &mut |reason| rejected.push(reason))
        .expect("recover when the overflowing byte is LF");
    assert_eq!(rejected, [Rejection::Oversize]);
    assert!(matches!(
        greenhouse_protocol::parse_host_line(&output),
        Ok(HostFrame::Assigned { device_id: 0, .. })
    ));
    assert_eq!(
        sql(&dir.path().join("db"), "SELECT count(*) FROM device"),
        1
    );
}
