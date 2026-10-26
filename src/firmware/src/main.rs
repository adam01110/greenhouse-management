#![no_std]
#![no_main]
#![feature(abi_avr_interrupt)]

use core::convert::Infallible;
use greenhouse_protocol::{Message, MoistureRaw};
use panic_halt as _;

mod uptime;

// Provisional simulator ID; server registration is not defined by this protocol.
const DEVICE_ID: u8 = 0;
const DATA_INTERVAL_MS: u32 = 5000;
const HEARTBEAT_INTERVAL_MS: u32 = 60_000;

#[arduino_hal::entry]
fn main() -> ! {
    #[expect(
        clippy::unwrap_used,
        reason = "Entry runs once, before any peripherals have been taken."
    )]
    let peripherals = arduino_hal::Peripherals::take().unwrap();
    let pins = arduino_hal::pins!(peripherals);

    let mut serial = arduino_hal::default_serial!(peripherals, pins, 57600);
    let mut adc = arduino_hal::Adc::new(
        peripherals.ADC,
        arduino_hal::adc::AdcSettings {
            ref_voltage: arduino_hal::adc::ReferenceVoltage::AVcc,
            ..Default::default()
        },
    );
    let moisture = pins.a0.into_analog_input(&mut adc);

    let mut pump = pins.d8.into_output();
    // Keep the simulated pump off until watering rules are agreed.
    pump.set_low();

    let mut led = pins.d13.into_output();

    uptime::init(peripherals.TC0);

    // SAFETY: Peripheral and ISR state is initialized; this is outside any critical section.
    #[expect(unsafe_code, reason = "Timer0 needs globally enabled interrupts.")]
    unsafe {
        avr_device::interrupt::enable();
    }

    led.toggle();
    let mut last_toggle = uptime::millis();
    let mut last_data = last_toggle.wrapping_sub(DATA_INTERVAL_MS);
    let mut last_heartbeat = last_toggle;

    loop {
        let now = uptime::millis();

        if now.wrapping_sub(last_toggle) >= 1000 {
            led.toggle();
            last_toggle = now;
        }

        if now.wrapping_sub(last_data) >= DATA_INTERVAL_MS {
            let raw = moisture.analog_read(&mut adc);
            send_data(&mut serial, raw, uptime::millis(), pump.is_set_high());
            last_data = now;
        }

        if now.wrapping_sub(last_heartbeat) >= HEARTBEAT_INTERVAL_MS {
            Message::Heartbeat {
                device_id: DEVICE_ID,
                uptime_ms: uptime::millis(),
            }
            .write_to(&mut serial)
            .unwrap_or_else(|never| match never {});
            last_heartbeat = now;
        }
    }
}

fn send_data(
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
