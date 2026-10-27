use crate::{ReceivedAtMs, Store, StoreError};
use greenhouse_protocol::{DeviceFrame, DeviceToken, HostFrame, Message};
use std::{
    collections::HashMap,
    io::{self, Write},
    time::{SystemTime, UNIX_EPOCH},
};
use thiserror::Error;

mod framing;
#[cfg(test)]
mod tests;
use framing::{Decoder, Frame};

/// A process-local handle, unrelated to the serial port's name.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct SessionId(u64);

/// Supplies the actual host reception time.
pub trait Clock {
    /// Returns a checked host timestamp.
    ///
    /// # Errors
    /// Returns a clock error if a valid timestamp cannot be obtained.
    fn received_at(&mut self) -> Result<ReceivedAtMs, ClockError>;
}

#[derive(Debug, Error)]
pub enum ClockError {
    #[error("host clock precedes the Unix epoch: {0}")]
    BeforeEpoch(#[from] std::time::SystemTimeError),
    #[error("host timestamp outside SQLite range: {0}")]
    InvalidTime(#[from] StoreError),
}

/// Reads the system wall clock for each accepted message.
pub struct SystemClock;
impl Clock for SystemClock {
    fn received_at(&mut self) -> Result<ReceivedAtMs, ClockError> {
        let elapsed = SystemTime::now().duration_since(UNIX_EPOCH)?;
        let millis =
            i128::try_from(elapsed.as_millis()).map_err(|_error| StoreError::InvalidTime)?;
        Ok(ReceivedAtMs::new(millis)?)
    }
}

/// Nonfatal input rejections. Processing continues with the next frame.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Rejection {
    Malformed,
    Oversize,
    Unregistered,
    WrongId,
    DuplicateToken,
}

#[derive(Debug, Error)]
pub enum IngestError {
    #[error("unknown session")]
    InvalidSession,
    #[error("session ID space exhausted")]
    SessionExhausted,
    #[error("a registered session changed its token")]
    TokenChanged,
    #[error("clock: {0}")]
    Clock(#[from] ClockError),
    #[error("store: {0}")]
    Store(#[from] StoreError),
    #[error("reply: {0}")]
    Io(#[from] io::Error),
}

struct Binding {
    token: DeviceToken,
    id: u8,
}
struct Session {
    decoder: Decoder,
    binding: Option<Binding>,
}

/// Synchronous single-owner ingestion. A token identifies a board but does not authenticate it.
/// Live token claims are scoped to this instance; all sources must share it.
pub struct Ingestor<C> {
    store: Store,
    clock: C,
    provision: bool,
    next: Option<u64>,
    sessions: HashMap<SessionId, Session>,
}

impl<C: Clock> Ingestor<C> {
    #[must_use]
    pub fn new(store: Store, clock: C, allow_provision: bool) -> Self {
        Self {
            store,
            clock,
            provision: allow_provision,
            next: Some(0),
            sessions: HashMap::new(),
        }
    }
    /// Starts a fresh source. Input is discarded until the first LF to regain framing.
    ///
    /// # Errors
    /// Returns an error if the process-local handle space is exhausted.
    pub fn open_session(&mut self) -> Result<SessionId, IngestError> {
        let value = self.next.ok_or(IngestError::SessionExhausted)?;
        self.next = value.checked_add(1);
        let id = SessionId(value);
        self.sessions.insert(
            id,
            Session {
                decoder: Decoder::new(),
                binding: None,
            },
        );
        Ok(id)
    }
    /// Releases any token claim and discards partial input; repeated closes are harmless.
    pub fn close_session(&mut self, id: SessionId) {
        self.sessions.remove(&id);
    }

    /// Processes all bytes, reporting nonfatal rejected frames through the callback.
    /// Fatal errors invalidate the session; the caller must disconnect that source.
    ///
    /// # Errors
    /// Returns an error for an invalid session, changed token, clock, storage or reply failure.
    pub fn feed<W: Write>(
        &mut self,
        id: SessionId,
        bytes: &[u8],
        reply: &mut W,
        reject: &mut impl FnMut(Rejection),
    ) -> Result<(), IngestError> {
        let mut session = self
            .sessions
            .remove(&id)
            .ok_or(IngestError::InvalidSession)?;
        self.process(&mut session, bytes, reply, reject)?;
        self.sessions.insert(id, session);
        Ok(())
    }

    fn process<W: Write>(
        &mut self,
        session: &mut Session,
        bytes: &[u8],
        reply: &mut W,
        reject: &mut impl FnMut(Rejection),
    ) -> Result<(), IngestError> {
        for &byte in bytes {
            if let Some(frame) = session.decoder.push(byte) {
                self.frame(session, &frame, reply, reject)?;
            }
        }
        Ok(())
    }

    fn frame<W: Write>(
        &mut self,
        session: &mut Session,
        frame: &Frame,
        reply: &mut W,
        reject: &mut impl FnMut(Rejection),
    ) -> Result<(), IngestError> {
        match frame {
            Frame::Line(Ok(DeviceFrame::Register { token })) => {
                self.accept_registration(session, *token, reply, reject)
            }
            Frame::Line(Ok(DeviceFrame::Telemetry(message))) => {
                self.telemetry(session, *message, reject)
            }
            Frame::Line(Err(_error)) => {
                reject(Rejection::Malformed);
                Ok(())
            }
            Frame::Oversize => {
                reject(Rejection::Oversize);
                Ok(())
            }
        }
    }

    fn accept_registration<W: Write>(
        &mut self,
        session: &mut Session,
        token: DeviceToken,
        reply: &mut W,
        reject: &mut impl FnMut(Rejection),
    ) -> Result<(), IngestError> {
        if self.accept_token(session, token, reject)? {
            self.register(session, token, reply)
        } else {
            Ok(())
        }
    }

    fn register<W: Write>(
        &mut self,
        session: &mut Session,
        token: DeviceToken,
        reply: &mut W,
    ) -> Result<(), IngestError> {
        let received = self.clock.received_at()?;
        let id = self.store.register(token, received, self.provision)?;
        send_assignment(reply, token, id)?;
        session.binding = Some(Binding { token, id });
        Ok(())
    }

    fn accept_token(
        &self,
        session: &Session,
        token: DeviceToken,
        reject: &mut impl FnMut(Rejection),
    ) -> Result<bool, IngestError> {
        if session
            .binding
            .as_ref()
            .is_some_and(|binding| binding.token != token)
        {
            return Err(IngestError::TokenChanged);
        }
        if self.sessions.values().any(|other| {
            other
                .binding
                .as_ref()
                .is_some_and(|binding| binding.token == token)
        }) {
            reject(Rejection::DuplicateToken);
            return Ok(false);
        }
        Ok(true)
    }

    fn telemetry(
        &mut self,
        session: &Session,
        message: Message,
        reject: &mut impl FnMut(Rejection),
    ) -> Result<(), IngestError> {
        let Some(binding) = &session.binding else {
            reject(Rejection::Unregistered);
            return Ok(());
        };
        let id = match message {
            Message::Data { device_id, .. } | Message::Heartbeat { device_id, .. } => device_id,
        };
        if id != binding.id {
            reject(Rejection::WrongId);
            return Ok(());
        }
        self.store.record(message, self.clock.received_at()?)?;
        Ok(())
    }
}

struct Reply<'a, W>(&'a mut W);
impl<W: Write> ufmt::uWrite for Reply<'_, W> {
    type Error = io::Error;
    fn write_str(&mut self, value: &str) -> Result<(), Self::Error> {
        self.0.write_all(value.as_bytes())
    }
}

fn send_assignment<W: Write>(reply: &mut W, token: DeviceToken, id: u8) -> io::Result<()> {
    HostFrame::Assigned {
        token,
        device_id: id,
    }
    .write_to(&mut Reply(reply))
}
