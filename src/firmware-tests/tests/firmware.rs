use std::{
    cell::RefCell,
    ffi::{CString, c_void},
    path::PathBuf,
    ptr::{self, NonNull},
};

use object::{Object, ObjectSymbol};
use simavr_ffi as ffi;

const CLOCK_HZ: u32 = 16_000_000;
const LED_CYCLES: u64 = 16_000_000;
const LED_TOLERANCE: u64 = 512;
const SIMULATION_LIMIT: u64 = 40_000_000;
const RAM_START: u32 = 0x100;
const MILLIS_NEAR_WRAP: u32 = u32::MAX - 499;

struct Simulator {
    cpu: NonNull<ffi::avr_t>,
    _firmware: Box<ffi::elf_firmware_t>,
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
        Self {
            cpu,
            _firmware: firmware,
        }
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

#[derive(Default)]
struct TelemetryState {
    adc_input: *mut ffi::avr_irq_t,
    input_mv: u32,
    line: Vec<u8>,
    frames: Vec<String>,
    data_times: Vec<u32>,
    heartbeat_times: Vec<u32>,
    pump_high: bool,
    line_overflow: bool,
}

unsafe extern "C" fn adc_requested(_: *mut ffi::avr_irq_t, _: u32, parameter: *mut c_void) {
    // SAFETY: parameter points to the live TelemetryState; callback executes
    // synchronously while avr_run is called on the test thread.
    let state = unsafe { &*parameter.cast::<RefCell<TelemetryState>>() }.borrow();
    // SAFETY: adc_input is a valid ADC0 IRQ retained by the simulator.
    unsafe { ffi::avr_raise_irq(state.adc_input, state.input_mv) };
}

unsafe extern "C" fn uart_byte(_: *mut ffi::avr_irq_t, value: u32, parameter: *mut c_void) {
    // SAFETY: parameter points to the live TelemetryState; callback executes
    // synchronously while avr_run is called on the test thread.
    let mut state = unsafe { &*parameter.cast::<RefCell<TelemetryState>>() }.borrow_mut();
    if state.line.len() >= 39 {
        state.line_overflow = true;
        return;
    }
    let byte = value.to_le_bytes()[0];
    state.line.push(byte);
    if byte == b'\n' {
        if state.line.len() < 2 || state.line[state.line.len() - 2] != b'\r' {
            state.line_overflow = true;
        }
        let frame = String::from_utf8_lossy(&state.line).into_owned();
        state.frames.push(frame);
        state.line.clear();
    }
}

unsafe extern "C" fn pump_changed(_: *mut ffi::avr_irq_t, value: u32, parameter: *mut c_void) {
    // SAFETY: parameter points to the live TelemetryState; callback executes
    // synchronously while avr_run is called on the test thread.
    let mut state = unsafe { &*parameter.cast::<RefCell<TelemetryState>>() }.borrow_mut();
    state.pump_high |= value & 1 != 0;
}

#[test]
fn telemetry_adc_uart_timing_and_pump_output() {
    let mut simulator = Simulator::load();
    let state = RefCell::new(TelemetryState::default());
    let adc_input = irq(
        &simulator,
        ioctl(b'a', b'd', b'c', b'0'),
        ffi::ADC_IRQ_ADC0.cast_signed(),
    );
    state.borrow_mut().adc_input = adc_input.as_ptr();
    let adc_trigger = irq(
        &simulator,
        ioctl(b'a', b'd', b'c', b'0'),
        ffi::ADC_IRQ_OUT_TRIGGER.cast_signed(),
    );
    let uart = irq(
        &simulator,
        ioctl(b'u', b'a', b'r', b'0'),
        ffi::UART_IRQ_OUTPUT.cast_signed(),
    );
    let pump = irq(
        &simulator,
        ioctl(b'i', b'o', b'g', b'B'),
        ffi::IOPORT_IRQ_PIN0.cast_signed(),
    );
    let _adc_subscription = register_notify(adc_trigger, Some(adc_requested), &state);
    let _uart_subscription = register_notify(uart, Some(uart_byte), &state);
    let _pump_subscription = register_notify(pump, Some(pump_changed), &state);

    let deadline = 992_000_000;
    while state.borrow().heartbeat_times.is_empty() && simulator.cycle() < deadline {
        simulator.step();
        process_frames(&mut state.borrow_mut());
    }
    process_frames(&mut state.borrow_mut());
    let state = state.borrow();
    assert_eq!(state.heartbeat_times.len(), 1, "expected one heartbeat");
    assert_eq!(state.data_times.len(), 13, "expected 13 DATA frames");
    assert!(state.line.is_empty(), "UART ended with an incomplete frame");
    assert!(
        !state.line_overflow,
        "UART line exceeded 39 bytes or lacked CRLF"
    );
    assert!(!state.pump_high, "D8/PB0 went HIGH");
    assert_ne!(simulator.read(0x24) & 1, 0, "DDRB0 must be an output");
    assert_eq!(simulator.read(0x25) & 1, 0, "PORTB0 must be held LOW");
}

fn process_frames(state: &mut TelemetryState) {
    while let Some(frame) = state.frames.pop() {
        let Some(frame) = frame.strip_suffix("\r\n") else {
            panic!("UART frame did not end in CRLF: {frame:?}");
        };
        let fields: Vec<_> = frame.split(',').collect();
        match fields.as_slice() {
            ["DATA", device, uptime, raw, pump] => {
                assert_eq!(device.parse::<u32>().unwrap(), 0, "device identity");
                let uptime = uptime.parse::<u32>().unwrap();
                let raw = raw.parse::<u32>().unwrap();
                assert_eq!(pump.parse::<u32>().unwrap(), 0, "commanded pump state");
                assert!(raw <= 1023, "ADC value exceeds 10-bit range");
                if state.data_times.is_empty() {
                    assert!(uptime <= 1, "initial DATA was not sampled promptly");
                } else {
                    let elapsed = uptime.wrapping_sub(*state.data_times.last().unwrap());
                    assert!(
                        (5000..=5001).contains(&elapsed),
                        "DATA cadence: {elapsed} ms"
                    );
                }
                let phase = state.data_times.len() % 3;
                match phase {
                    0 => assert_eq!(raw, 0, "ADC0 at 0 mV"),
                    1 => assert!((511..=512).contains(&raw), "ADC0 at 2500 mV: {raw}"),
                    2 => assert_eq!(raw, 1023, "ADC0 at 5000 mV"),
                    _ => unreachable!(),
                }
                state.data_times.push(uptime);
                state.input_mv = match state.data_times.len() % 3 {
                    0 => 0,
                    1 => 2500,
                    _ => 5000,
                };
            }
            ["HB", device, uptime] => {
                assert_eq!(device.parse::<u32>().unwrap(), 0, "heartbeat identity");
                let uptime = uptime.parse::<u32>().unwrap();
                assert!(
                    (60_000..=60_010).contains(&uptime),
                    "heartbeat uptime: {uptime}"
                );
                state.heartbeat_times.push(uptime);
            }
            _ => panic!("unexpected/non-protocol UART line: {frame:?}"),
        }
    }
}
