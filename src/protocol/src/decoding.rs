use crate::{DeviceFrame, DeviceToken, HostFrame, Message, MoistureRaw};

/// An invalid complete protocol line.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ParseError {
    InvalidFrame,
    InvalidValue,
}

type Fields<'a> = core::str::Split<'a, char>;

fn fields(line: &[u8], max_len: usize) -> Result<Fields<'_>, ParseError> {
    if line.len() > max_len {
        return Err(ParseError::InvalidFrame);
    }
    let body = line.strip_suffix(b"\r\n").ok_or(ParseError::InvalidFrame)?;
    let text = core::str::from_utf8(body).map_err(|_utf8| ParseError::InvalidFrame)?;
    if !text.is_ascii() {
        return Err(ParseError::InvalidFrame);
    }
    Ok(text.split(','))
}

fn next<'a>(parts: &mut Fields<'a>) -> Result<&'a str, ParseError> {
    parts.next().ok_or(ParseError::InvalidFrame)
}

fn number<T: core::str::FromStr>(field: &str) -> Result<T, ParseError> {
    if field.is_empty() || !field.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(ParseError::InvalidValue);
    }
    field.parse().map_err(|_number| ParseError::InvalidValue)
}

fn next_number<T: core::str::FromStr>(parts: &mut Fields<'_>) -> Result<T, ParseError> {
    number(next(parts)?)
}

fn data(parts: &mut Fields<'_>) -> Result<Message, ParseError> {
    let device_id = next_number(parts)?;
    let uptime_ms = next_number(parts)?;
    let moisture_raw = moisture(parts)?;
    let pump_on = pump(parts)?;
    Ok(Message::Data {
        device_id,
        uptime_ms,
        moisture_raw,
        pump_on,
    })
}

fn moisture(parts: &mut Fields<'_>) -> Result<MoistureRaw, ParseError> {
    MoistureRaw::new(next_number(parts)?).ok_or(ParseError::InvalidValue)
}

fn pump(parts: &mut Fields<'_>) -> Result<bool, ParseError> {
    match next(parts)? {
        "0" => Ok(false),
        "1" => Ok(true),
        _ => Err(ParseError::InvalidValue),
    }
}

fn heartbeat(parts: &mut Fields<'_>) -> Result<Message, ParseError> {
    Ok(Message::Heartbeat {
        device_id: next_number(parts)?,
        uptime_ms: next_number(parts)?,
    })
}

fn token(parts: &mut Fields<'_>) -> Result<DeviceToken, ParseError> {
    DeviceToken::parse(next(parts)?)
}

fn end(parts: &mut Fields<'_>) -> Result<(), ParseError> {
    if parts.next().is_some() {
        Err(ParseError::InvalidFrame)
    } else {
        Ok(())
    }
}

fn device(parts: &mut Fields<'_>) -> Result<DeviceFrame, ParseError> {
    match parts.next() {
        Some("REG") => token(parts).map(|token| DeviceFrame::Register { token }),
        Some("DATA") => data(parts).map(DeviceFrame::Telemetry),
        Some("HB") => heartbeat(parts).map(DeviceFrame::Telemetry),
        _ => Err(ParseError::InvalidFrame),
    }
}

fn assignment(parts: &mut Fields<'_>) -> Result<HostFrame, ParseError> {
    if next(parts)? != "ID" {
        return Err(ParseError::InvalidFrame);
    }
    Ok(HostFrame::Assigned {
        token: token(parts)?,
        device_id: next_number(parts)?,
    })
}

/// Parses one complete device line, including CRLF.
///
/// # Errors
/// Rejects malformed frames, invalid fields and out-of-range values.
pub fn parse_device_line(line: &[u8]) -> Result<DeviceFrame, ParseError> {
    let mut parts = fields(line, 38)?;
    let result = device(&mut parts)?;
    end(&mut parts)?;
    Ok(result)
}

/// Parses one complete server line, including CRLF.
///
/// # Errors
/// Rejects malformed frames, invalid fields and out-of-range values.
pub fn parse_host_line(line: &[u8]) -> Result<HostFrame, ParseError> {
    let mut parts = fields(line, 41)?;
    let result = assignment(&mut parts)?;
    end(&mut parts)?;
    Ok(result)
}
