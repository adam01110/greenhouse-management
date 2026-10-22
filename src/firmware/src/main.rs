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

    let serial = arduino_hal::default_serial!(peripherals, pins, 57600);
    greenhouse_firmware::put_console(serial);
    greenhouse_firmware::print!("Uno ");
    greenhouse_firmware::println!("blink ready at {} baud", 57600u32);
    greenhouse_firmware::println!();

    let mut led = pins.d13.into_output();

    loop {
        led.toggle();
        arduino_hal::delay_ms(1000);
    }
}
