use std::{cell::RefCell, ffi::c_void, ops::RangeInclusive};

use super::frame::{self, Frame, Heartbeat, Sample};
use crate::{Simulator, Subscription, ffi, ioctl, irq, register_notify};

pub(super) const ADC_INPUTS: [(u32, RangeInclusive<u16>); 3] =
    [(0, 0..=0), (2500, 511..=512), (5000, 1023..=1023)];
const MAX_FRAME_BYTES: usize = 39;
const HEARTBEAT_DEADLINE: u64 = 62 * 16_000_000;

#[derive(Default)]
pub(super) struct TelemetryState {
    adc_input: *mut ffi::avr_irq_t,
    input_mv: u32,
    line: Vec<u8>,
    frames: Vec<Vec<u8>>,
    pump_high: bool,
    line_overflow: bool,
}

#[derive(Default)]
pub(super) struct Recording {
    pub samples: Vec<Sample>,
    pub heartbeats: Vec<Heartbeat>,
}

pub(super) struct TelemetryProbe {
    state: RefCell<TelemetryState>,
}

impl TelemetryProbe {
    pub fn new(simulator: &Simulator) -> Self {
        let adc_input = irq(
            simulator,
            ioctl(b'a', b'd', b'c', b'0'),
            ffi::ADC_IRQ_ADC0.cast_signed(),
        );
        Self {
            state: RefCell::new(TelemetryState {
                adc_input: adc_input.as_ptr(),
                input_mv: ADC_INPUTS[0].0,
                ..TelemetryState::default()
            }),
        }
    }

    pub fn observe(&self, simulator: &Simulator) -> [Subscription<'_, RefCell<TelemetryState>>; 3] {
        let adc = irq(
            simulator,
            ioctl(b'a', b'd', b'c', b'0'),
            ffi::ADC_IRQ_OUT_TRIGGER.cast_signed(),
        );
        let uart = irq(
            simulator,
            ioctl(b'u', b'a', b'r', b'0'),
            ffi::UART_IRQ_OUTPUT.cast_signed(),
        );
        let pump = irq(
            simulator,
            ioctl(b'i', b'o', b'g', b'B'),
            ffi::IOPORT_IRQ_PIN0.cast_signed(),
        );
        [
            register_notify(adc, Some(adc_requested), &self.state),
            register_notify(uart, Some(uart_byte), &self.state),
            register_notify(pump, Some(pump_changed), &self.state),
        ]
    }

    pub fn record_until_heartbeat(&self, simulator: &mut Simulator) -> Recording {
        let mut recording = Recording::default();
        while recording.heartbeats.is_empty() && simulator.cycle() < HEARTBEAT_DEADLINE {
            simulator.step();
            self.collect_frames(&mut recording);
        }
        recording
    }

    fn collect_frames(&self, recording: &mut Recording) {
        let mut state = self.state.borrow_mut();
        for bytes in std::mem::take(&mut state.frames) {
            match frame::parse(&bytes) {
                Frame::Data(sample) => {
                    recording.samples.push(sample);
                    state.input_mv = ADC_INPUTS[recording.samples.len() % ADC_INPUTS.len()].0;
                }
                Frame::Heartbeat(heartbeat) => recording.heartbeats.push(heartbeat),
            }
        }
    }

    pub fn assert_uart_complete(&self) {
        let state = self.state.borrow();
        assert!(state.line.is_empty(), "UART ended with an incomplete frame");
        assert!(
            !state.line_overflow,
            "UART frame exceeded {MAX_FRAME_BYTES} bytes"
        );
    }

    pub fn assert_pump_stayed_off(&self, simulator: &Simulator) {
        const DDRB: usize = 0x24;
        const PORTB: usize = 0x25;
        assert!(!self.state.borrow().pump_high, "D8/PB0 went HIGH");
        assert_ne!(simulator.read(DDRB) & 1, 0, "D8 must be an output");
        assert_eq!(simulator.read(PORTB) & 1, 0, "D8 must be driven LOW");
    }
}

unsafe extern "C" fn adc_requested(_: *mut ffi::avr_irq_t, _: u32, parameter: *mut c_void) {
    // SAFETY: parameter points to the live TelemetryState; callback executes
    // synchronously while avr_run is called on the test thread.
    let state = unsafe { &*parameter.cast::<RefCell<TelemetryState>>() }.borrow();
    // SAFETY: adc_input is a valid ADC0 IRQ retained by the simulator.
    unsafe { ffi::avr_raise_irq(state.adc_input, state.input_mv) };
}

unsafe extern "C" fn uart_byte(_: *mut ffi::avr_irq_t, value: u32, parameter: *mut c_void) {
    // SAFETY: the subscription keeps this RefCell alive throughout synchronous callbacks.
    let mut state = unsafe { &*parameter.cast::<RefCell<TelemetryState>>() }.borrow_mut();
    if state.line.len() >= MAX_FRAME_BYTES {
        state.line_overflow = true;
        return;
    }

    let byte = value.to_le_bytes()[0];
    state.line.push(byte);
    if byte == b'\n' {
        let frame = std::mem::take(&mut state.line);
        state.frames.push(frame);
    }
}

unsafe extern "C" fn pump_changed(_: *mut ffi::avr_irq_t, value: u32, parameter: *mut c_void) {
    // SAFETY: parameter points to the live TelemetryState; callback executes
    // synchronously while avr_run is called on the test thread.
    let mut state = unsafe { &*parameter.cast::<RefCell<TelemetryState>>() }.borrow_mut();
    state.pump_high |= value & 1 != 0;
}
