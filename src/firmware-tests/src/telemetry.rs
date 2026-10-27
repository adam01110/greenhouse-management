use std::{cell::RefCell, ffi::c_void};

use super::{Simulator, ffi, ioctl, irq, register_notify};

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
