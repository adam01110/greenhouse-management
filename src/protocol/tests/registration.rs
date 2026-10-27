use greenhouse_protocol::{
    parse_device_line, parse_host_line, DeviceFrame, DeviceToken, HostFrame, Message, MoistureRaw,
};

// Bytes pair the hexadecimal digits in TOKEN; the mixed pattern exercises every digit.
const TOKEN_BYTES: [u8; 16] = [
    0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef, 0xfe, 0xdc, 0xba, 0x98, 0x76, 0x54, 0x32, 0x10,
];
const TOKEN: &str = "0123456789abcdeffedcba9876543210";

#[derive(Debug, Eq, PartialEq)]
struct WriteError(usize);

struct Writer {
    bytes: Vec<u8>,
    limit: usize,
}

impl ufmt::uWrite for Writer {
    type Error = WriteError;

    fn write_str(&mut self, value: &str) -> Result<(), Self::Error> {
        for byte in value.bytes() {
            if self.bytes.len() == self.limit {
                return Err(WriteError(self.limit));
            }
            self.bytes.push(byte);
        }

        Ok(())
    }
}

#[test]
fn token_roundtrip_and_byte_access() {
    let token = DeviceToken::new(TOKEN_BYTES);

    assert_eq!(token.as_bytes(), &TOKEN_BYTES);
    assert_eq!(DeviceToken::parse(TOKEN), Ok(token));

    // Reject empty, short, long, uppercase, non-hex and non-ASCII tokens.
    for invalid in [
        "",
        "0123456789abcdeffedcba987654321",
        "0123456789abcdeffedcba98765432100",
        "0123456789ABCDEffedcba9876543210",
        "0123456789abcdeffedcba987654321g",
        "é123456789abcdeffedcba987654321",
    ] {
        assert!(DeviceToken::parse(invalid).is_err(), "{invalid}");
    }
}

#[test]
fn register_and_assignment_roundtrip_and_max_lengths() {
    let token = DeviceToken::new(TOKEN_BYTES);
    let reg = DeviceFrame::Register { token };
    let expected = format!("REG,{TOKEN}\r\n");
    let mut writer = Writer {
        bytes: Vec::new(),
        limit: usize::MAX,
    };

    assert_eq!(expected.len(), DeviceFrame::MAX_LEN);
    assert_eq!(reg.write_to(&mut writer), Ok(()));
    assert_eq!(writer.bytes, expected.as_bytes());
    assert_eq!(parse_device_line(&writer.bytes), Ok(reg));

    for id in [0, 255] {
        let assignment = HostFrame::Assigned {
            token,
            device_id: id,
        };
        let expected = format!("ID,{TOKEN},{id}\r\n");

        if id == 255 {
            assert_eq!(expected.len(), HostFrame::MAX_LEN);
        }

        let mut writer = Writer {
            bytes: Vec::new(),
            limit: usize::MAX,
        };

        assert_eq!(assignment.write_to(&mut writer), Ok(()));
        assert_eq!(writer.bytes, expected.as_bytes());
        assert_eq!(parse_host_line(&writer.bytes), Ok(assignment));
    }
}

#[test]
fn registration_writer_failures_preserve_error_and_prefix() {
    let token = DeviceToken::new(TOKEN_BYTES);
    let expected = format!("REG,{TOKEN}\r\n");

    // Fail after every possible byte and verify both the error and written prefix.
    for limit in 0..expected.len() {
        let mut writer = Writer {
            bytes: Vec::new(),
            limit,
        };
        assert_eq!(
            DeviceFrame::Register { token }.write_to(&mut writer),
            Err(WriteError(limit))
        );
        assert_eq!(writer.bytes, expected.as_bytes().get(..limit).unwrap());
    }

    let expected = format!("ID,{TOKEN},255\r\n");

    for limit in 0..expected.len() {
        let mut writer = Writer {
            bytes: Vec::new(),
            limit,
        };
        assert_eq!(
            HostFrame::Assigned {
                token,
                device_id: 255
            }
            .write_to(&mut writer),
            Err(WriteError(limit))
        );
        assert_eq!(writer.bytes, expected.as_bytes().get(..limit).unwrap());
    }
}

#[test]
fn telemetry_roundtrip_at_limits() {
    let messages = [
        Message::Data {
            device_id: 0,
            uptime_ms: 0,
            moisture_raw: MoistureRaw::new(0).unwrap(),
            pump_on: false,
        },
        Message::Data {
            device_id: 255,
            uptime_ms: u32::MAX,
            moisture_raw: MoistureRaw::new(1023).unwrap(),
            pump_on: true,
        },
        Message::Heartbeat {
            device_id: 0,
            uptime_ms: 0,
        },
        Message::Heartbeat {
            device_id: 255,
            uptime_ms: u32::MAX,
        },
    ];

    for message in messages {
        let frame = DeviceFrame::Telemetry(message);
        let mut writer = Writer {
            bytes: Vec::new(),
            limit: usize::MAX,
        };

        assert_eq!(frame.write_to(&mut writer), Ok(()));
        assert_eq!(parse_device_line(&writer.bytes), Ok(frame));
    }
}

#[test]
fn rejects_bad_device_lines() {
    // Reject malformed framing, numeric bounds, invalid fields and host-only frames.
    for line in [
        &b"HB,0000000000000000000000000000000000,0\r\n"[..],
        &b"HB,0,0\n"[..],
        b"HB,0,0\r",
        b"HB,0,0\r\n\r\n",
        b"HB,0,0,extra\r\n",
        b"HB,0\r\n",
        b"HB,256,0\r\n",
        b"HB,0,4294967296\r\n",
        b"HB,+1,0\r\n",
        b"HB,-1,0\r\n",
        b"HB, 1,0\r\n",
        b"HB,,0\r\n",
        b"HB,0,\xff\r\n",
        b"DATA,0,0,1024,0\r\n",
        b"DATA,0,0,65536,0\r\n",
        b"DATA,0,0,0,2\r\n",
        b"DATA,0,0,0,01\r\n",
        b"DATA,0,0,0\r\n",
        b"DATA,0,0,0,0,1\r\n",
        b"REG,0123456789ABCDEffedcba9876543210\r\n",
        b"REG,0123456789abcdeffedcba9876543210,0\r\n",
        b"ID,0123456789abcdeffedcba9876543210,0\r\n",
    ] {
        assert!(parse_device_line(line).is_err(), "{line:?}");
    }
}

#[test]
fn rejects_bad_host_lines() {
    // Reject invalid IDs, malformed framing, bad tokens and device-only frames.
    for line in [
        &b"ID,0123456789abcdeffedcba9876543210,0000\r\n"[..],
        &b"ID,0123456789abcdeffedcba9876543210,256\r\n"[..],
        b"ID,0123456789abcdeffedcba9876543210,+1\r\n",
        b"ID,0123456789abcdeffedcba9876543210,-1\r\n",
        b"ID,0123456789abcdeffedcba9876543210, 1\r\n",
        b"ID,0123456789abcdeffedcba9876543210,1\n",
        b"ID,0123456789abcdeffedcba9876543210,1,2\r\n",
        b"ID,0123456789abcdeffedcba9876543210\r\n",
        b"ID,0123456789ABCDEffedcba9876543210,1\r\n",
        b"ID,0123456789abcdeffedcba9876543210,\xff\r\n",
        b"REG,0123456789abcdeffedcba9876543210\r\n",
    ] {
        assert!(parse_host_line(line).is_err(), "{line:?}");
    }
}
