#![no_std]
#![feature(abi_avr_interrupt)]

mod console;
mod telemetry;
mod uptime;

pub use console::put_console;
#[doc(hidden)]
pub use console::{__ufmt, __with_console};

const DATA_INTERVAL_MS: u32 = 5000;
const HEARTBEAT_INTERVAL_MS: u32 = 60_000;

pub fn run(peripherals: arduino_hal::Peripherals) -> ! {
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
            telemetry::send_data(&mut serial, raw, uptime::millis(), pump.is_set_high());
            last_data = now;
        }

        if now.wrapping_sub(last_heartbeat) >= HEARTBEAT_INTERVAL_MS {
            telemetry::send_heartbeat(&mut serial, uptime::millis());
            last_heartbeat = now;
        }
    }
}
