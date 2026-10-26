#![no_std]

//! Shared telemetry types and allocation-free UART encoding.
//!
//! Frames contain comma-separated ASCII fields and end with CRLF.

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

    #[must_use]
    pub const fn get(self) -> u16 {
        self.0
    }
}

/// Device telemetry with wrapping millisecond uptime.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Message {
    Data {
        /// Device identifier, independent of the serial port.
        device_id: u8,
        /// Device uptime in milliseconds, wrapping after `u32::MAX`.
        uptime_ms: u32,
        moisture_raw: MoistureRaw,
        /// Reported output command, not physical pump feedback.
        pump_on: bool,
    },
    Heartbeat {
        /// Device identifier, independent of the serial port.
        device_id: u8,
        /// Device uptime in milliseconds, wrapping after `u32::MAX`.
        uptime_ms: u32,
    },
}

impl Message {
    /// Streams one complete frame, including CRLF, without buffering it.
    ///
    /// # Errors
    ///
    /// Returns the writer's first error unchanged. A failed write may have
    /// already emitted a partial frame; output is not rolled back.
    pub fn write_to<W: ufmt::uWrite>(&self, writer: &mut W) -> Result<(), W::Error> {
        match self {
            Self::Data {
                device_id,
                uptime_ms,
                moisture_raw,
                pump_on,
            } => ufmt::uwrite!(
                writer,
                "DATA,{},{},{},{}\r\n",
                device_id,
                uptime_ms,
                moisture_raw.get(),
                u8::from(*pump_on)
            ),
            Self::Heartbeat {
                device_id,
                uptime_ms,
            } => ufmt::uwrite!(writer, "HB,{},{}\r\n", device_id, uptime_ms),
        }
    }
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
