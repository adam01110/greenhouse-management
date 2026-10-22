#![no_std]
#![no_main]

use panic_halt as _;

#[arduino_hal::entry]
fn main() -> ! {
    let peripherals = arduino_hal::Peripherals::take().unwrap();
    let pins = arduino_hal::pins!(peripherals);

    let mut led = pins.d13.into_output();
    led.set_low();

    avr_device::interrupt::disable();
    // SM=010 selects power-down; SE enables the sleep instruction.
    peripherals
        .CPU
        .smcr()
        .write(|w| unsafe { w.sm().bits(0b010).se().set_bit() });

    loop {
        avr_device::asm::sleep();
    }
}
