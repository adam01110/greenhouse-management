use greenhouse_protocol::{Message, MoistureRaw};

#[derive(Debug, Eq, PartialEq)]
struct WriteError(usize);

struct Writer {
    bytes: [u8; 28],
    written: usize,
    limit: usize,
}

impl Writer {
    const fn new(limit: usize) -> Self {
        Self {
            bytes: [0; 28],
            written: 0,
            limit,
        }
    }

    fn bytes(&self) -> Option<&[u8]> {
        self.bytes.get(..self.written)
    }
}

impl ufmt::uWrite for Writer {
    type Error = WriteError;

    fn write_str(&mut self, value: &str) -> Result<(), Self::Error> {
        for byte in value.bytes() {
            if self.written == self.limit {
                return Err(WriteError(self.written));
            }
            let slot = self
                .bytes
                .get_mut(self.written)
                .ok_or(WriteError(self.written))?;
            *slot = byte;
            self.written += 1;
        }
        Ok(())
    }
}

fn assert_frame(message: Message, expected: &[u8]) {
    let mut writer = Writer::new(expected.len());
    assert_eq!(message.write_to(&mut writer), Ok(()));
    assert_eq!(writer.bytes(), Some(expected));
}

#[test]
fn data_minimum_fields() {
    assert_frame(
        Message::Data {
            device_id: 0,
            uptime_ms: 0,
            moisture_raw: MoistureRaw::new(0).unwrap(),
            pump_on: false,
        },
        b"DATA,0,0,0,0\r\n",
    );
}

#[test]
fn data_maximum_fields() {
    assert_frame(
        Message::Data {
            device_id: u8::MAX,
            uptime_ms: u32::MAX,
            moisture_raw: MoistureRaw::new(1023).unwrap(),
            pump_on: true,
        },
        b"DATA,255,4294967295,1023,1\r\n",
    );
}

#[test]
fn heartbeat_minimum_fields() {
    assert_frame(
        Message::Heartbeat {
            device_id: 0,
            uptime_ms: 0,
        },
        b"HB,0,0\r\n",
    );
}

#[test]
fn heartbeat_maximum_fields() {
    assert_frame(
        Message::Heartbeat {
            device_id: u8::MAX,
            uptime_ms: u32::MAX,
        },
        b"HB,255,4294967295\r\n",
    );
}

#[test]
fn data_failure_at_every_byte_preserves_error_and_prefix() {
    assert_failures(
        Message::Data {
            device_id: u8::MAX,
            uptime_ms: u32::MAX,
            moisture_raw: MoistureRaw::new(1023).unwrap(),
            pump_on: true,
        },
        b"DATA,255,4294967295,1023,1\r\n",
    );
}

#[test]
fn heartbeat_failure_at_every_byte_preserves_error_and_prefix() {
    assert_failures(
        Message::Heartbeat {
            device_id: u8::MAX,
            uptime_ms: u32::MAX,
        },
        b"HB,255,4294967295\r\n",
    );
}

fn assert_failures(message: Message, expected: &[u8]) {
    for limit in 0..expected.len() {
        let mut writer = Writer::new(limit);
        assert_eq!(message.write_to(&mut writer), Err(WriteError(limit)));
        assert_eq!(writer.bytes(), expected.get(..limit));
    }
}
