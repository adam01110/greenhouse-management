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
    greenhouse_firmware::run(peripherals)
}
