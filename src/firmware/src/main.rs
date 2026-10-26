#![no_std]
#![no_main]
#![feature(abi_avr_interrupt)]

use panic_halt as _;

mod uptime;

#[arduino_hal::entry]
fn main() -> ! {
    #[expect(
        clippy::unwrap_used,
        reason = "Entry runs once, before any peripherals have been taken."
    )]
    let peripherals = arduino_hal::Peripherals::take().unwrap();
    let pins = arduino_hal::pins!(peripherals);

    let mut serial = arduino_hal::default_serial!(peripherals, pins, 57600);
    ufmt::uwrite!(&mut serial, "Uno blink ready at {} baud\r\n\r\n", 57600u32)
        .unwrap_or_else(|never| match never {});

    let mut led = pins.d13.into_output();
    uptime::init(peripherals.TC0);
    // SAFETY: Peripheral and ISR state is initialized; this is outside any critical section.
    #[expect(unsafe_code, reason = "Timer0 needs globally enabled interrupts.")]
    unsafe {
        avr_device::interrupt::enable();
    }

    led.toggle();
    let mut last_toggle = uptime::millis();
    loop {
        let now = uptime::millis();
        if now.wrapping_sub(last_toggle) >= 1000 {
            led.toggle();
            last_toggle = now;
        }
    }
}
