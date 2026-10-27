use greenhouse_protocol::{DeviceFrame, ParseError, parse_device_line};

pub(super) enum Frame {
    Line(Result<DeviceFrame, ParseError>),
    Oversize,
}

pub(super) struct Decoder {
    bytes: [u8; DeviceFrame::MAX_LEN],
    len: usize,
    discarding: bool,
}
impl Decoder {
    pub(super) const fn new() -> Self {
        Self {
            bytes: [0; DeviceFrame::MAX_LEN],
            len: 0,
            discarding: true,
        }
    }
    pub(super) fn push(&mut self, byte: u8) -> Option<Frame> {
        if self.discarding {
            if byte == b'\n' {
                self.discarding = false;
            }
            return None;
        }
        let Some(slot) = self.bytes.get_mut(self.len) else {
            self.len = 0;
            self.discarding = byte != b'\n';
            return Some(Frame::Oversize);
        };
        *slot = byte;
        self.len += 1;
        if byte != b'\n' {
            return None;
        }
        let line = self
            .bytes
            .get(..self.len)
            .map_or(Err(ParseError::InvalidValue), parse_device_line);
        self.len = 0;
        Some(Frame::Line(line))
    }
}
