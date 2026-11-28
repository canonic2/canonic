//! Bounded little-endian JSON framing for the pinned Playwright pipe protocol.
//!
//! Keep the reader in one owned task. Dropping a pending read preserves framing
//! progress; dropping a pending write poisons that writer because dispatch may
//! have begun. Neither transport retries bytes or classifies action outcomes.

use futures_util::{
    SinkExt, StreamExt,
    stream::{SplitSink, SplitStream},
};
use serde_json::Value;
use std::io::{self, Write};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio_tungstenite::tungstenite::Message;

/// Maximum UTF-8 JSON payload, excluding the four-byte header.
pub const MAX_FRAME_BYTES: usize = 64 * 1024 * 1024;

/// Framing failures are terminal for incoming data and partially sent messages.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Driver frame limit must be between 1 and {MAX_FRAME_BYTES} bytes")]
    InvalidLimit,
    #[error("Driver frame exceeds the {limit}-byte limit")]
    Oversized { limit: usize },
    #[error("Driver sent an empty frame")]
    Empty,
    #[error("Driver stream ended within a frame")]
    Truncated,
    #[error("Driver frame contains invalid JSON at line {line}, column {column}")]
    InvalidJson { line: usize, column: usize },
    #[error("Driver message must be a JSON object")]
    NotObject,
    #[error("Driver transport is closed or failed")]
    Closed,
    #[error("Driver transport I/O failed")]
    Io(#[from] io::Error),
    #[error("Playwright server WebSocket failed")]
    WebSocket,
}

pub(super) trait ReadFrame {
    fn read_value(&mut self) -> impl Future<Output = Result<Option<Value>, Error>> + Send;
}

pub(super) trait WriteFrame {
    fn write_value(&mut self, value: &Value) -> impl Future<Output = Result<(), Error>> + Send;
}

impl<R: AsyncRead + Unpin + Send> ReadFrame for Reader<R> {
    async fn read_value(&mut self) -> Result<Option<Value>, Error> {
        self.read().await
    }
}

impl<W: AsyncWrite + Unpin + Send> WriteFrame for Writer<W> {
    async fn write_value(&mut self, value: &Value) -> Result<(), Error> {
        self.write(value).await
    }
}

impl ReadFrame for SplitStream<super::attachment::Socket> {
    async fn read_value(&mut self) -> Result<Option<Value>, Error> {
        while let Some(message) = self.next().await {
            match message.map_err(|_| Error::WebSocket)? {
                Message::Text(text) => {
                    let value: Value =
                        serde_json::from_str(&text).map_err(|error| Error::InvalidJson {
                            line: error.line(),
                            column: error.column(),
                        })?;
                    return if value.is_object() {
                        Ok(Some(value))
                    } else {
                        Err(Error::NotObject)
                    };
                }
                Message::Close(_) => return Ok(None),
                Message::Ping(_) | Message::Pong(_) => continue,
                _ => return Err(Error::WebSocket),
            }
        }
        Ok(None)
    }
}

impl WriteFrame for SplitSink<super::attachment::Socket, Message> {
    async fn write_value(&mut self, value: &Value) -> Result<(), Error> {
        let payload = encode(value, MAX_FRAME_BYTES)?;
        let payload = String::from_utf8(payload).expect("Serialized JSON is UTF-8");
        self.send(Message::Text(payload.into()))
            .await
            .map_err(|_| Error::WebSocket)
    }
}

fn validate_limit(limit: usize) -> Result<(), Error> {
    if (1..=MAX_FRAME_BYTES).contains(&limit) {
        Ok(())
    } else {
        Err(Error::InvalidLimit)
    }
}

/// A single bounded incoming frame with persistent partial-read state.
pub struct Reader<R> {
    io: R,
    limit: usize,
    header: [u8; 4],
    header_read: usize,
    payload: Vec<u8>,
    payload_read: usize,
    closed: bool,
}

impl<R: AsyncRead + Unpin> Reader<R> {
    pub fn new(io: R) -> Self {
        Self::with_limit(io, MAX_FRAME_BYTES).expect("Default frame limit is valid")
    }

    /// A caller may lower the cap, but cannot exceed the binding's hard limit.
    pub fn with_limit(io: R, limit: usize) -> Result<Self, Error> {
        validate_limit(limit)?;
        Ok(Self {
            io,
            limit,
            header: [0; 4],
            header_read: 0,
            payload: Vec::new(),
            payload_read: 0,
            closed: false,
        })
    }

    /// Returns `None` only for clean EOF at a frame boundary. Any error poisons
    /// the reader; subsequent calls fail without consuming further bytes.
    pub async fn read(&mut self) -> Result<Option<Value>, Error> {
        if self.closed {
            return Err(Error::Closed);
        }
        let result = self.read_frame().await;
        if !matches!(result, Ok(Some(_))) {
            self.closed = true;
        }
        result
    }

    async fn read_frame(&mut self) -> Result<Option<Value>, Error> {
        while self.header_read < self.header.len() {
            let count = self.io.read(&mut self.header[self.header_read..]).await?;
            if count == 0 {
                return if self.header_read == 0 {
                    Ok(None)
                } else {
                    Err(Error::Truncated)
                };
            }
            self.header_read += count;
        }
        if self.payload.is_empty() {
            let length = u32::from_le_bytes(self.header) as usize;
            if length == 0 {
                return Err(Error::Empty);
            }
            if length > self.limit {
                return Err(Error::Oversized { limit: self.limit });
            }
            // The untrusted length has been checked before allocating.
            self.payload.resize(length, 0);
        }
        while self.payload_read < self.payload.len() {
            let count = self.io.read(&mut self.payload[self.payload_read..]).await?;
            if count == 0 {
                return Err(Error::Truncated);
            }
            self.payload_read += count;
        }
        let value: Value =
            serde_json::from_slice(&self.payload).map_err(|error| Error::InvalidJson {
                line: error.line(),
                column: error.column(),
            })?;
        if !value.is_object() {
            return Err(Error::NotObject);
        }
        self.header_read = 0;
        self.payload_read = 0;
        // Do not retain a large frame allocation between messages.
        self.payload = Vec::new();
        Ok(Some(value))
    }
}

/// A single outgoing lane. The owner must register pending calls before writing
/// and must retain dispatch uncertainty if this operation is cancelled or fails.
pub struct Writer<W> {
    io: W,
    limit: usize,
    failed: bool,
}

impl<W: AsyncWrite + Unpin> Writer<W> {
    pub fn new(io: W) -> Self {
        Self::with_limit(io, MAX_FRAME_BYTES).expect("Default frame limit is valid")
    }

    pub fn with_limit(io: W, limit: usize) -> Result<Self, Error> {
        validate_limit(limit)?;
        Ok(Self {
            io,
            limit,
            failed: false,
        })
    }

    /// Encode within the cap before dispatch. Local validation errors leave the
    /// writer usable. Cancellation or I/O failure after admission poisons it.
    pub async fn write(&mut self, value: &Value) -> Result<(), Error> {
        if self.failed {
            return Err(Error::Closed);
        }
        let payload = encode(value, self.limit)?;
        self.failed = true;
        self.io
            .write_all(&(payload.len() as u32).to_le_bytes())
            .await?;
        self.io.write_all(&payload).await?;
        self.io.flush().await?;
        self.failed = false;
        Ok(())
    }
}

fn encode(value: &Value, limit: usize) -> Result<Vec<u8>, Error> {
    if !value.is_object() {
        return Err(Error::NotObject);
    }
    let mut buffer = LimitedBuffer {
        bytes: Vec::new(),
        limit,
    };
    serde_json::to_writer(&mut buffer, value).map_err(|_| Error::Oversized { limit })?;
    Ok(buffer.bytes)
}

struct LimitedBuffer {
    bytes: Vec<u8>,
    limit: usize,
}

impl Write for LimitedBuffer {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() > self.limit - self.bytes.len() {
            return Err(io::Error::other("Driver frame limit exceeded"));
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests;
