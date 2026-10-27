use super::{REG, feed_ok, setup, sql, sync};
use crate::{IngestError, ReceivedAtMs};
use greenhouse_protocol::{DeviceToken, HostFrame};
use std::io::{self, Write};

struct FailingWriter {
    remaining: usize,
}
impl Write for FailingWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if self.remaining == 0 {
            return Err(io::Error::new(io::ErrorKind::BrokenPipe, "injected"));
        }
        let count = self.remaining.min(buf.len());
        self.remaining -= count;
        Ok(count)
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
#[test]
fn partial_ack_failure_reconnects_to_same_id() {
    for offset in 0..39 {
        let (dir, mut ingest) = setup(true);
        let a = ingest.open_session().expect("a");
        sync(&mut ingest, a);
        let mut writer = FailingWriter { remaining: offset };
        assert!(matches!(
            ingest.feed(a, REG, &mut writer, &mut |_| {}),
            Err(IngestError::Io(_))
        ));
        assert_eq!(
            sql(&dir.path().join("db"), "SELECT count(*) FROM device"),
            1
        );
        let b = ingest.open_session().expect("b");
        sync(&mut ingest, b);
        let mut reply = Vec::new();
        ingest.feed(b, REG, &mut reply, &mut |_| {}).expect("retry");
        assert_eq!(
            greenhouse_protocol::parse_host_line(&reply),
            Ok(HostFrame::Assigned {
                token: DeviceToken::new([0; 16]),
                device_id: 0
            })
        );
        assert_eq!(
            sql(&dir.path().join("db"), "SELECT count(*) FROM device"),
            1
        );
    }
}

struct InspectWriter {
    path: std::path::PathBuf,
    output: Vec<u8>,
}
impl Write for InspectWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if self.output.is_empty() && sql(&self.path, "SELECT count(*) FROM device") != 1 {
            return Err(io::Error::other("registration not committed"));
        }
        self.output.extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
#[test]
fn committed_before_first_reply_byte() {
    let (dir, mut ingest) = setup(true);
    let id = ingest.open_session().expect("open");
    sync(&mut ingest, id);
    let mut writer = InspectWriter {
        path: dir.path().join("db"),
        output: Vec::new(),
    };
    ingest
        .feed(id, REG, &mut writer, &mut |_| {})
        .expect("register");
    greenhouse_protocol::parse_host_line(&writer.output).expect("response");
}

#[test]
fn every_maximum_width_ack_failure_retries_the_same_persisted_id() {
    let (dir, mut ingest) = setup(true);
    for value in 1..=u8::MAX {
        ingest
            .store
            .register(
                DeviceToken::new([value; 16]),
                ReceivedAtMs::new(1).expect("valid time"),
                true,
            )
            .expect("reserve preceding IDs");
    }
    let expected = b"ID,00000000000000000000000000000000,255\r\n";
    assert_eq!(expected.len(), HostFrame::MAX_LEN);
    for offset in 0..expected.len() {
        let failed = ingest.open_session().expect("open failing connection");
        sync(&mut ingest, failed);
        let mut writer = FailingWriter { remaining: offset };
        assert!(matches!(
            ingest.feed(failed, REG, &mut writer, &mut |_| {}),
            Err(IngestError::Io(_))
        ));
        assert!(matches!(
            ingest.feed(failed, b"HB,255,0\r\n", &mut Vec::new(), &mut |_| {}),
            Err(IngestError::InvalidSession)
        ));
        let retried = ingest.open_session().expect("open retry connection");
        sync(&mut ingest, retried);
        assert_eq!(feed_ok(&mut ingest, retried, REG), expected);
        feed_ok(&mut ingest, retried, b"HB,255,0\r\n");
        ingest.close_session(retried);
        assert_eq!(
            sql(&dir.path().join("db"), "SELECT count(*) FROM device"),
            256
        );
    }
    let success = ingest.open_session().expect("open final connection");
    sync(&mut ingest, success);
    ingest
        .feed(
            success,
            REG,
            &mut FailingWriter {
                remaining: expected.len(),
            },
            &mut |_| {},
        )
        .expect("exact full-frame budget must succeed");
    feed_ok(&mut ingest, success, b"HB,255,0\r\n");
}

struct InterruptedByteWriter {
    output: Vec<u8>,
    interrupt: bool,
}
impl Write for InterruptedByteWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if self.interrupt {
            self.interrupt = false;
            return Err(io::Error::from(io::ErrorKind::Interrupted));
        }
        self.interrupt = true;
        let Some(&byte) = bytes.first() else {
            return Ok(0);
        };
        self.output.push(byte);
        Ok(1)
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[test]
fn short_and_interrupted_writes_preserve_the_complete_assignment() {
    let (dir, mut ingest) = setup(true);
    let id = ingest.open_session().expect("open source");
    sync(&mut ingest, id);
    let mut writer = InterruptedByteWriter {
        output: Vec::new(),
        interrupt: true,
    };
    ingest
        .feed(id, REG, &mut writer, &mut |_| {})
        .expect("retry short and interrupted writes");
    assert_eq!(writer.output, b"ID,00000000000000000000000000000000,0\r\n");
    feed_ok(&mut ingest, id, b"HB,0,1\r\n");
    assert_eq!(
        sql(&dir.path().join("db"), "SELECT count(*) FROM heartbeat"),
        1
    );
}

#[test]
fn zero_capacity_writer_invalidates_the_source_but_keeps_the_committed_id() {
    let (dir, mut ingest) = setup(true);
    let id = ingest.open_session().expect("open source");
    sync(&mut ingest, id);
    let mut storage: [u8; 0] = [];
    let mut writer = io::Cursor::new(storage.as_mut_slice());
    assert!(matches!(ingest.feed(id, REG, &mut writer, &mut |_| {}),
        Err(IngestError::Io(ref source)) if source.kind() == io::ErrorKind::WriteZero));
    assert!(matches!(
        ingest.feed(id, b"HB,0,0\r\n", &mut Vec::new(), &mut |_| {}),
        Err(IngestError::InvalidSession)
    ));
    assert_eq!(
        sql(&dir.path().join("db"), "SELECT count(*) FROM device"),
        1
    );
    assert_eq!(
        sql(&dir.path().join("db"), "SELECT count(*) FROM heartbeat"),
        0
    );
    let retry = ingest.open_session().expect("open retry connection");
    sync(&mut ingest, retry);
    assert!(matches!(
        greenhouse_protocol::parse_host_line(&feed_ok(&mut ingest, retry, REG)),
        Ok(HostFrame::Assigned { device_id: 0, .. })
    ));
}
