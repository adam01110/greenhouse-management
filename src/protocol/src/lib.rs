#![no_std]

//! Shared telemetry types for the firmware and host.
//!
//! These are in-memory values, not a wire encoding. The UART protocol uses
//! comma-separated ASCII fields with CRLF line endings.

/// A raw 10-bit soil-moisture ADC reading, not a calibrated percentage.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MoistureRaw(u16);

impl MoistureRaw {
    /// Returns `None` when the reading exceeds the ADC's 0–1023 range.
    #[must_use]
    pub const fn new(value: u16) -> Option<Self> {
        match value <= 1023 {
            true => Some(Self(value)),
            false => None,
        }
    }

    /// Returns the uncalibrated ADC value.
    #[must_use]
    pub const fn get(self) -> u16 {
        self.0
    }
}

/// Telemetry sent by a device; error messages are not defined yet.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Message {
    /// A `DATA` frame containing a measurement and the current pump state.
    Data {
        /// Device identifier, independent of the serial port.
        device_id: u8,
        /// Device uptime in milliseconds, wrapping after `u32::MAX`.
        uptime_ms: u32,
        /// Uncalibrated soil-moisture ADC reading.
        moisture_raw: MoistureRaw,
        /// Whether the firmware reports the pump as on (wire value 1) or off (0).
        pump_on: bool,
    },
    /// An `HB` frame reporting device liveness without a measurement.
    Heartbeat {
        /// Device identifier, independent of the serial port.
        device_id: u8,
        /// Device uptime in milliseconds, wrapping after `u32::MAX`.
        uptime_ms: u32,
    },
}

#[cfg(test)]
mod tests {
    use super::MoistureRaw;

    #[test]
    fn adc_accepts_every_ten_bit_value() {
        for value in 0..=1023 {
            assert_eq!(MoistureRaw::new(value).map(MoistureRaw::get), Some(value));
        }
    }

    #[test]
    fn adc_rejects_every_out_of_range_value() {
        for value in 1024..=u16::MAX {
            assert_eq!(MoistureRaw::new(value), None);
        }
    }
}
