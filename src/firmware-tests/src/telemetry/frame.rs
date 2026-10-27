#[derive(Debug)]
pub(super) struct Sample {
    pub device_id: u8,
    pub uptime_ms: u32,
    pub raw: u16,
    pub pump: u8,
}

#[derive(Debug)]
pub(super) struct Heartbeat {
    pub device_id: u8,
    pub uptime_ms: u32,
}

pub(super) enum Frame {
    Data(Sample),
    Heartbeat(Heartbeat),
}

pub(super) fn parse(bytes: &[u8]) -> Frame {
    let line = std::str::from_utf8(bytes).expect("UART frame must be UTF-8 ASCII");
    let line = line
        .strip_suffix("\r\n")
        .expect("UART frame must end with CRLF");
    let fields: Vec<_> = line.split(',').collect();

    match fields.as_slice() {
        ["DATA", device, uptime, raw, pump] => Frame::Data(Sample {
            device_id: device.parse().expect("DATA device ID"),
            uptime_ms: uptime.parse().expect("DATA uptime"),
            raw: raw.parse().expect("DATA raw ADC reading"),
            pump: pump.parse().expect("DATA pump command"),
        }),
        ["HB", device, uptime] => Frame::Heartbeat(Heartbeat {
            device_id: device.parse().expect("HB device ID"),
            uptime_ms: uptime.parse().expect("HB uptime"),
        }),
        _ => panic!("unexpected UART frame: {line:?}"),
    }
}

#[test]
fn rejects_malformed_frames() {
    let invalid: &[&[u8]] = &[
        b"DATA,0,0,512,0\n",
        b"DATA,0,0,512\r\n",
        b"DATA,0,0,512,0,extra\r\n",
        b"DATA,0,0,\xff,0\r\n",
        b"DATA,256,0,512,0\r\n",
        b"DATA,0,4294967296,512,0\r\n",
        b"DATA,0,0,65536,0\r\n",
        b"DATA,0,0,512,256\r\n",
        b"HB,0,not-a-number\r\n",
        b"startup banner\r\n",
    ];

    for bytes in invalid {
        assert!(
            std::panic::catch_unwind(|| parse(bytes)).is_err(),
            "accepted {bytes:?}"
        );
    }
}
