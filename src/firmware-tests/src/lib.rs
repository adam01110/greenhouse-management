#![cfg(test)]

use std::{
    ffi::CString,
    path::PathBuf,
    ptr::{self, NonNull},
};

use simavr_ffi as ffi;

mod telemetry;
mod timer;

const CLOCK_HZ: u32 = 16 * 1000 * 1000;

struct Simulator {
    cpu: NonNull<ffi::avr_t>,
    #[expect(dead_code, reason = "simavr borrows firmware data until termination")]
    firmware: Box<ffi::elf_firmware_t>,
}

impl Simulator {
    fn load() -> Self {
        let firmware_path = firmware_path();
        let path = CString::new(firmware_path.to_string_lossy().as_bytes())
            .expect("firmware path has no NUL byte");

        // SAFETY: zero is the documented empty initialization for elf_firmware_t.
        let mut firmware = Box::new(unsafe { std::mem::zeroed() });

        // SAFETY: path is NUL-terminated and firmware is writable for the call.
        assert_eq!(
            unsafe { ffi::elf_read_firmware(path.as_ptr(), &raw mut *firmware) },
            0
        );

        let mcu = c"atmega328p";
        // SAFETY: mcu is a static NUL-terminated MCU name.
        let cpu = NonNull::new(unsafe { ffi::avr_make_mcu_by_name(mcu.as_ptr()) })
            .expect("ATmega328P simulator is available");

        // SAFETY: cpu is a newly allocated simulator instance.
        assert_eq!(unsafe { ffi::avr_init(cpu.as_ptr()) }, 0);

        // SAFETY: cpu and firmware remain alive for the simulator lifetime.
        unsafe {
            (*cpu.as_ptr()).frequency = CLOCK_HZ;
            (*cpu.as_ptr()).vcc = 5000;
            (*cpu.as_ptr()).avcc = 5000;
            (*cpu.as_ptr()).aref = 5000;
            ffi::avr_load_firmware(cpu.as_ptr(), &raw mut *firmware);
        }

        Self { cpu, firmware }
    }

    fn cycle(&self) -> u64 {
        // SAFETY: cpu remains initialized and is only accessed on this test thread.
        unsafe { (*self.cpu.as_ptr()).cycle }
    }

    fn step(&mut self) {
        // SAFETY: cpu is initialized and all IRQ callbacks use valid state pointers.
        let state = unsafe { ffi::avr_run(self.cpu.as_ptr()) }.cast_unsigned();
        assert_ne!(state, ffi::cpu_Done, "firmware stopped unexpectedly");
        assert_ne!(state, ffi::cpu_Crashed, "firmware crashed");
    }

    fn ram_end(&self) -> usize {
        // SAFETY: cpu remains initialized for this adapter's lifetime.
        usize::from(unsafe { (*self.cpu.as_ptr()).ramend })
    }

    fn read(&self, address: usize) -> u8 {
        assert!(address <= self.ram_end());
        // SAFETY: address is inside allocated simulator data memory.
        unsafe { *(*self.cpu.as_ptr()).data.add(address) }
    }

    fn write(&mut self, address: usize, value: u8) {
        assert!(address <= self.ram_end());
        // SAFETY: address is inside allocated simulator data memory.
        unsafe { *(*self.cpu.as_ptr()).data.add(address) = value };
    }

    fn sreg_interrupts_enabled(&self) -> bool {
        // SAFETY: initialized ATmega328P exposes the AVR status register array.
        unsafe { (*self.cpu.as_ptr()).sreg[ffi::S_I as usize] != 0 }
    }
}

impl Drop for Simulator {
    fn drop(&mut self) {
        // SAFETY: cpu was initialized once and is terminated once here.
        unsafe { ffi::avr_terminate(self.cpu.as_ptr()) };
    }
}

fn firmware_path() -> PathBuf {
    let profile = std::env::var("GH_FIRMWARE_PROFILE").unwrap_or_else(|_| "debug".to_owned());
    assert!(
        matches!(profile.as_str(), "debug" | "release"),
        "invalid GH_FIRMWARE_PROFILE"
    );
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../firmware/target/avr-none")
        .join(profile)
        .join("greenhouse-firmware.elf")
}

fn ioctl(a: u8, b: u8, c: u8, d: u8) -> u32 {
    (u32::from(a) << 24) | (u32::from(b) << 16) | (u32::from(c) << 8) | u32::from(d)
}

fn irq(cpu: &Simulator, ioctl: u32, index: i32) -> NonNull<ffi::avr_irq_t> {
    // SAFETY: cpu is initialized; ioctl and index identify a simavr IRQ.
    NonNull::new(unsafe { ffi::avr_io_getirq(cpu.cpu.as_ptr(), ioctl, index) })
        .expect("requested simavr IRQ exists")
}

struct Subscription<'a, T> {
    signal: NonNull<ffi::avr_irq_t>,
    callback: ffi::avr_irq_notify_t,
    context: &'a T,
}

impl<T> Drop for Subscription<'_, T> {
    fn drop(&mut self) {
        // SAFETY: remove the registered callback while its borrowed context is still alive.
        unsafe {
            ffi::avr_irq_unregister_notify(
                self.signal.as_ptr(),
                self.callback,
                ptr::from_ref(self.context).cast_mut().cast(),
            );
        }
    }
}

fn register_notify<T>(
    signal: NonNull<ffi::avr_irq_t>,
    callback: ffi::avr_irq_notify_t,
    context: &T,
) -> Subscription<'_, T> {
    // SAFETY: these callbacks use this context type; the subscription keeps it borrowed.
    unsafe {
        ffi::avr_irq_register_notify(
            signal.as_ptr(),
            callback,
            ptr::from_ref(context).cast_mut().cast(),
        );
    }
    Subscription {
        signal,
        callback,
        context,
    }
}
