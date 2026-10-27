use crate::ParseError;

/// Stable 16-byte provisioned identity, encoded on the wire as 32 lowercase
/// hexadecimal digits and echoed in the server assignment. It is not a secret
/// and does not authenticate the device.
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
            // Two hexadecimal digits form the high and low four bits of a byte.
            *byte = (hex(*high)? << 4) | hex(*low)?;
        }

        Ok(Self(bytes))
    }

    pub(crate) fn write_to<W: ufmt::uWrite>(&self, writer: &mut W) -> Result<(), W::Error> {
        for byte in self.0 {
            // Split the byte into two base-16 digits.
            write_digit(writer, byte >> 4)?;
            write_digit(writer, byte % 16)?;
        }

        Ok(())
    }
}

const fn hex(byte: u8) -> Result<u8, ParseError> {
    // ASCII digits start at zero; letters start at the hexadecimal value ten.
    match byte {
        b'0'..=b'9' => Ok(byte - b'0'),
        b'a'..=b'f' => Ok(byte - b'a' + 10),
        _ => Err(ParseError::InvalidValue),
    }
}

fn write_digit<W: ufmt::uWrite>(writer: &mut W, value: u8) -> Result<(), W::Error> {
    let mut encoded = [0; 4];
    // Both callers pass a four-bit value, so the result is an ASCII hex digit.
    let digit = if value < 10 {
        b'0' + value
    } else {
        b'a' + value - 10
    };
    let digit = char::from(digit);

    writer.write_str(digit.encode_utf8(&mut encoded))
}
