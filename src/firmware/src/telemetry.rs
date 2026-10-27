use core::convert::Infallible;
use greenhouse_protocol::{Message, MoistureRaw};

// Provisional simulator ID; server registration is not defined by this protocol.
const DEVICE_ID: u8 = 0;

pub(super) fn send_data(
    serial: &mut impl ufmt::uWrite<Error = Infallible>,
    raw: u16,
    uptime_ms: u32,
    pump_on: bool,
) {
    let Some(moisture_raw) = MoistureRaw::new(raw) else {
        return;
    };

    Message::Data {
        device_id: DEVICE_ID,
        uptime_ms,
        moisture_raw,
        pump_on,
    }
    .write_to(serial)
    .unwrap_or_else(|never| match never {});
}

pub(super) fn send_heartbeat(serial: &mut impl ufmt::uWrite<Error = Infallible>, uptime_ms: u32) {
    Message::Heartbeat {
        device_id: DEVICE_ID,
        uptime_ms,
    }
    .write_to(serial)
    .unwrap_or_else(|never| match never {});
}
