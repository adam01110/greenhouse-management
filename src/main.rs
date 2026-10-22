#![no_std]
#![no_main]

use panic_halt as _;

#[arduino_hal::entry]
fn main() -> ! {
    let peripherals = arduino_hal::Peripherals::take().unwrap();
    let pins = arduino_hal::pins!(peripherals);

    let serial = arduino_hal::default_serial!(peripherals, pins, 57600);
    uno_blink::put_console(serial);
    uno_blink::print!("Uno ");
    uno_blink::println!("blink ready at {} baud", 57600u32);
    uno_blink::println!();

    let mut led = pins.d13.into_output();

    loop {
        led.toggle();
        arduino_hal::delay_ms(1000);
    }
}
