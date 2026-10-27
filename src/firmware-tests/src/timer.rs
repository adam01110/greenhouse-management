use object::{Object, ObjectSymbol};
use std::{cell::RefCell, ffi::c_void};

use super::{Simulator, ffi, firmware_path, ioctl, irq, register_notify};

const LED_CYCLES: u64 = 16_000_000;
const LED_TOLERANCE: u64 = 512;
const SIMULATION_LIMIT: u64 = 40_000_000;
const RAM_START: u32 = 0x100;
const MILLIS_NEAR_WRAP: u32 = u32::MAX - 499;

#[derive(Default)]
struct EdgeState {
    last_level: u32,
    edges: Vec<u64>,
}

struct LedContext {
    cpu: *mut ffi::avr_t,
    state: RefCell<EdgeState>,
}

unsafe extern "C" fn led_changed(_: *mut ffi::avr_irq_t, value: u32, parameter: *mut c_void) {
    // SAFETY: parameter points to the live LedContext registered below; simavr
    // invokes this callback synchronously while that context is borrowed.
    let context = unsafe { &*parameter.cast::<LedContext>() };
    let mut state = context.state.borrow_mut();
    let level = value & 1;
    if level != state.last_level && state.edges.len() < 3 {
        // SAFETY: cpu remains initialized until its callback is unregistered.
        state.edges.push(unsafe { (*context.cpu).cycle });
    }
    state.last_level = level;
}

fn led_edges(simulator: &mut Simulator, context: &LedContext, count: usize) -> Vec<u64> {
    context.state.borrow_mut().edges.clear();
    let deadline = simulator.cycle() + SIMULATION_LIMIT;
    while context.state.borrow().edges.len() < count && simulator.cycle() < deadline {
        simulator.step();
    }
    assert_eq!(
        context.state.borrow().edges.len(),
        count,
        "LED transitions missed simulation deadline"
    );
    context.state.borrow().edges.clone()
}

#[test]
fn timer_tick_led_cadence_and_millis_rollover() {
    let mut simulator = Simulator::load();
    let symbol_address = find_millis_symbol(&simulator);
    let led = LedContext {
        cpu: simulator.cpu.as_ptr(),
        state: RefCell::default(),
    };
    let led_signal = irq(
        &simulator,
        ioctl(b'i', b'o', b'g', b'B'),
        ffi::IOPORT_IRQ_PIN5.cast_signed(),
    );
    let _led_subscription = register_notify(led_signal, Some(led_changed), &led);

    let edges = led_edges(&mut simulator, &led, 3);
    for interval in edges.windows(2).map(|pair| pair[1] - pair[0]) {
        assert!(
            (LED_CYCLES - LED_TOLERANCE..=LED_CYCLES + LED_TOLERANCE).contains(&interval),
            "LED interval {interval} cycles differs from one second by more than 512 cycles"
        );
    }
    assert_eq!(simulator.read(0x47), 249, "OCR0A");
    assert_eq!(simulator.read(0x44), 2, "TCCR0A");
    assert_eq!(simulator.read(0x45), 3, "TCCR0B");
    assert_eq!(simulator.read(0x6e), 2, "TIMSK0");

    wait_for_interrupts(&mut simulator, "before rollover injection");
    for (offset, byte) in MILLIS_NEAR_WRAP.to_le_bytes().into_iter().enumerate() {
        simulator.write(symbol_address + offset, byte);
    }
    let edges = led_edges(&mut simulator, &led, 3);
    assert!(
        edges.windows(2).all(|pair| {
            (LED_CYCLES - LED_TOLERANCE..=LED_CYCLES + LED_TOLERANCE).contains(&(pair[1] - pair[0]))
        }),
        "LED cadence failed across millis rollover"
    );

    wait_for_interrupts(&mut simulator, "after rollover");
    let after_wrap = u32::from_le_bytes(std::array::from_fn(|offset| {
        simulator.read(symbol_address + offset)
    }));
    assert!(
        after_wrap <= 2000,
        "millisecond counter did not wrap: {after_wrap}"
    );
}

fn find_millis_symbol(simulator: &Simulator) -> usize {
    let bytes = std::fs::read(firmware_path()).expect("read firmware ELF");
    let image = object::File::parse(bytes.as_slice()).expect("parse firmware ELF");
    let addresses: Vec<_> = image
        .symbols()
        .filter(|symbol| symbol.name().is_ok_and(|name| name.contains("MILLIS")))
        .map(|symbol| symbol.address() & 0xffff)
        .collect();
    assert_eq!(addresses.len(), 1, "expected exactly one MILLIS ELF symbol");
    let address = usize::try_from(addresses[0]).expect("SRAM address fits usize");
    let ramend = simulator.ram_end();
    assert!(
        address >= RAM_START as usize && address + 3 <= ramend,
        "MILLIS is outside SRAM"
    );
    address
}

fn wait_for_interrupts(simulator: &mut Simulator, phase: &str) {
    let deadline = simulator.cycle() + 16_000;
    while !simulator.sreg_interrupts_enabled() && simulator.cycle() < deadline {
        simulator.step();
    }
    assert!(
        simulator.sreg_interrupts_enabled(),
        "interrupts remained disabled {phase}"
    );
}
