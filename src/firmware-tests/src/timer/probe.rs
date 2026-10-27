use object::{Object, ObjectSymbol};
use std::{cell::RefCell, ffi::c_void};

use crate::{ffi, firmware_path, ioctl, irq, register_notify, Simulator, Subscription};

const EDGE_DEADLINE_CYCLES: u64 = 40 * 1000 * 1000;

// ATmega328P SRAM starts at data-space address 0x100.
const RAM_START: usize = 0x100;

#[derive(Default)]
struct EdgeState {
    last_level: u32,
    edges: Vec<u64>,
}

pub(super) struct TimerProbe {
    cpu: *mut ffi::avr_t,
    state: RefCell<EdgeState>,
    millis_address: usize,
}

impl TimerProbe {
    pub fn new(simulator: &Simulator) -> Self {
        Self {
            cpu: simulator.cpu.as_ptr(),
            state: RefCell::default(),
            millis_address: find_millis_symbol(simulator),
        }
    }

    pub fn observe(&self, simulator: &Simulator) -> Subscription<'_, Self> {
        let led = irq(
            simulator,
            ioctl(b'i', b'o', b'g', b'B'),
            ffi::IOPORT_IRQ_PIN5.cast_signed(),
        );

        register_notify(led, Some(led_changed), self)
    }

    pub fn record_three_edges(&self, simulator: &mut Simulator) -> Vec<u64> {
        self.state.borrow_mut().edges.clear();
        let deadline = simulator.cycle() + EDGE_DEADLINE_CYCLES;

        while self.state.borrow().edges.len() < 3 && simulator.cycle() < deadline {
            simulator.step();
        }

        let edges = self.state.borrow().edges.clone();
        assert_eq!(edges.len(), 3, "LED transitions missed simulation deadline");

        edges
    }

    pub fn set_millis(&self, simulator: &mut Simulator, millis: u32) {
        wait_for_interrupts(simulator, "before rollover injection");

        for (offset, byte) in millis.to_le_bytes().into_iter().enumerate() {
            simulator.write(self.millis_address + offset, byte);
        }
    }

    pub fn millis(&self, simulator: &mut Simulator) -> u32 {
        wait_for_interrupts(simulator, "after rollover");

        u32::from_le_bytes(std::array::from_fn(|offset| {
            simulator.read(self.millis_address + offset)
        }))
    }
}

unsafe extern "C" fn led_changed(_: *mut ffi::avr_irq_t, value: u32, parameter: *mut c_void) {
    // SAFETY: parameter points to the live TimerProbe registered below; simavr
    // invokes this callback synchronously while that context is borrowed.
    let context = unsafe { &*parameter.cast::<TimerProbe>() };
    let mut state = context.state.borrow_mut();
    let level = value & 1;
    if level != state.last_level && state.edges.len() < 3 {
        // SAFETY: cpu remains initialized until its callback is unregistered.
        state.edges.push(unsafe { (*context.cpu).cycle });
    }
    state.last_level = level;
}

fn find_millis_symbol(simulator: &Simulator) -> usize {
    let bytes = std::fs::read(firmware_path()).expect("read firmware ELF");
    let image = object::File::parse(bytes.as_slice()).expect("parse firmware ELF");

    // AVR ELF data symbols include a segment prefix; SRAM uses the low 16 bits.
    let addresses: Vec<_> = image
        .symbols()
        .filter(|symbol| symbol.name().is_ok_and(|name| name.contains("MILLIS")))
        .map(|symbol| symbol.address() & 0xffff)
        .collect();

    assert_eq!(addresses.len(), 1, "expected exactly one MILLIS ELF symbol");
    let address = usize::try_from(addresses[0]).expect("SRAM address fits usize");
    let ramend = simulator.ram_end();

    assert!(
        address >= RAM_START && address + 3 <= ramend,
        "MILLIS is outside SRAM"
    );

    address
}

fn wait_for_interrupts(simulator: &mut Simulator, phase: &str) {
    let deadline = simulator.cycle() + 16 * 1000;

    while !simulator.sreg_interrupts_enabled() && simulator.cycle() < deadline {
        simulator.step();
    }

    assert!(
        simulator.sreg_interrupts_enabled(),
        "interrupts remained disabled {phase}"
    );
}
