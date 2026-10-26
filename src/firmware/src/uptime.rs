use avr_device::interrupt::{self, Mutex};
use core::cell::Cell;

const _: () = assert!(<arduino_hal::DefaultClock as arduino_hal::clock::Clock>::FREQ == 16_000_000);

static MILLIS: Mutex<Cell<u32>> = Mutex::new(Cell::new(0));

/// Reserves Timer0 for 1 ms ticks; call once before enabling interrupts.
#[expect(
    clippy::needless_pass_by_value,
    reason = "Taking Timer0 reserves it for uptime instead of allowing PWM reuse."
)]
pub fn init(timer: arduino_hal::pac::TC0) {
    timer.tccr0b().write(|w| w.cs0().no_clock());
    timer.timsk0().reset();
    timer.tccr0a().write(|w| w.wgm0().ctc());
    timer.tcnt0().write(|w| w.set(0));
    // CTC counts 0 through OCR: 16 MHz / 64 / (249 + 1) = 1 kHz.
    timer.ocr0a().write(|w| w.set(249));
    // OCF0A is write-one-to-clear; don't modify other pending flags.
    timer.tifr0().write(|w| w.ocf0a().set_bit());
    interrupt::free(|cs| MILLIS.borrow(cs).set(0));
    timer.timsk0().write(|w| w.ocie0a().set_bit());
    timer.tccr0b().write(|w| w.cs0().prescale_64());
}

/// Milliseconds since initialization, wrapping after `u32::MAX`.
/// Interrupts masked for a full tick or longer can cause lost ticks.
pub fn millis() -> u32 {
    interrupt::free(|cs| MILLIS.borrow(cs).get())
}

#[avr_device::interrupt(atmega328p)]
fn TIMER0_COMPA() {
    interrupt::free(|cs| {
        let millis = MILLIS.borrow(cs);
        millis.set(millis.get().wrapping_add(1));
    });
}
