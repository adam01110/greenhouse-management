use crate::{DeviceToken, Message};

/// A frame sent by a device.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DeviceFrame {
    Register { token: DeviceToken },
    Telemetry(Message),
}

/// A frame sent by the server.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HostFrame {
    Assigned { token: DeviceToken, device_id: u8 },
}

impl DeviceFrame {
    /// Maximum complete frame length in bytes, including CRLF.
    pub const MAX_LEN: usize = 38;

    /// Streams a complete frame, including CRLF.
    ///
    /// # Errors
    /// Returns the first writer error; already written bytes cannot be rolled back.
    pub fn write_to<W: ufmt::uWrite>(&self, writer: &mut W) -> Result<(), W::Error> {
        match self {
            Self::Register { token } => {
                writer.write_str("REG,")?;
                token.write_to(writer)?;
                writer.write_str("\r\n")
            }
            Self::Telemetry(message) => message.write_to(writer),
        }
    }
}

impl HostFrame {
    /// Maximum complete frame length in bytes, including CRLF.
    pub const MAX_LEN: usize = 41;

    /// Streams a complete frame, including CRLF.
    ///
    /// # Errors
    /// Returns the first writer error; already written bytes cannot be rolled back.
    pub fn write_to<W: ufmt::uWrite>(&self, writer: &mut W) -> Result<(), W::Error> {
        match self {
            Self::Assigned { token, device_id } => {
                writer.write_str("ID,")?;
                token.write_to(writer)?;
                ufmt::uwrite!(writer, ",{}\r\n", device_id)
            }
        }
    }
}
