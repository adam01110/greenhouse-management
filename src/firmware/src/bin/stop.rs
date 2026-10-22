#![no_std]
#![no_main]

use panic_halt as _;

#[arduino_hal::entry]
fn main() -> ! {
    #[expect(
        clippy::unwrap_used,
        reason = "Entry runs once, before any peripherals have been taken."
    )]
    let peripherals = arduino_hal::Peripherals::take().unwrap();
    let pins = arduino_hal::pins!(peripherals);

    let mut led = pins.d13.into_output();
    led.set_low();

    avr_device::interrupt::disable();
    peripherals.CPU.smcr().write(|w| {
        // SAFETY: 0b010 is the ATmega328P's power-down mode; SE enables sleep.
        #[expect(unsafe_code, reason = "The PAC exposes the sleep-mode bits as unsafe.")]
        unsafe {
            w.sm().bits(0b010).se().set_bit()
        }
    });

    loop {
        avr_device::asm::sleep();
    }
}
