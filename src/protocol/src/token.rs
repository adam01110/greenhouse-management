use crate::ParseError;

/// Provisioned device identifier. It does not authenticate the device.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DeviceToken([u8; 16]);

impl DeviceToken {
    #[must_use]
    pub const fn new(bytes: [u8; 16]) -> Self {
        Self(bytes)
    }

    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 16] {
        &self.0
    }

    /// Parses exactly 32 lowercase hexadecimal digits.
    ///
    /// # Errors
    /// Returns an error for invalid length or characters.
    pub fn parse(text: &str) -> Result<Self, ParseError> {
        if text.len() != 32 {
            return Err(ParseError::InvalidValue);
        }
        let mut bytes = [0; 16];
        for ([high, low], byte) in text
            .as_bytes()
            .as_chunks::<2>()
            .0
            .iter()
            .zip(bytes.iter_mut())
        {
            *byte = (hex(*high)? << 4) | hex(*low)?;
        }
        Ok(Self(bytes))
    }

    pub(crate) fn write_to<W: ufmt::uWrite>(&self, writer: &mut W) -> Result<(), W::Error> {
        for byte in self.0 {
            write_digit(writer, byte >> 4)?;
            write_digit(writer, byte & 0x0f)?;
        }
        Ok(())
    }
}

const fn hex(byte: u8) -> Result<u8, ParseError> {
    match byte {
        b'0'..=b'9' => Ok(byte - b'0'),
        b'a'..=b'f' => Ok(byte - b'a' + 10),
        _ => Err(ParseError::InvalidValue),
    }
}

fn write_digit<W: ufmt::uWrite>(writer: &mut W, value: u8) -> Result<(), W::Error> {
    let mut encoded = [0; 4];
    let digit = char::from_digit(u32::from(value), 16).unwrap_or('0');
    writer.write_str(digit.encode_utf8(&mut encoded))
}
