#![no_std]

//! Legacy console: writes block with interrupts masked; uninitialized output is dropped.

use avr_device::interrupt;
use core::cell::RefCell;

type Console = arduino_hal::hal::usart::Usart0<arduino_hal::DefaultClock>;

static CONSOLE: interrupt::Mutex<RefCell<Option<Console>>> =
    interrupt::Mutex::new(RefCell::new(None));

pub fn put_console(console: Console) {
    interrupt::free(|cs| {
        *CONSOLE.borrow(cs).borrow_mut() = Some(console);
    });
}

#[doc(hidden)]
pub use ufmt as __ufmt;

#[doc(hidden)]
pub fn __with_console(write: impl FnOnce(&mut Console)) {
    interrupt::free(|cs| {
        if let Some(console) = CONSOLE.borrow(cs).borrow_mut().as_mut() {
            write(console);
        }
    });
}

#[macro_export]
macro_rules! print {
    ($($t:tt)*) => {
        $crate::__with_console(|console| {
            let _ = $crate::__ufmt::uwrite!(console, $($t)*);
        })
    };
}

/// Ignores UART write errors. Formatted calls append LF; the empty form emits CRLF.
#[macro_export]
macro_rules! println {
    () => {
        $crate::print!("\r\n")
    };
    ($($t:tt)*) => {
        $crate::__with_console(|console| {
            let _ = $crate::__ufmt::uwriteln!(console, $($t)*);
        })
    };
}
