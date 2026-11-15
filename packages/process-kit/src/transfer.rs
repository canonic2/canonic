//! Bounded live-resource copies over an exclusively owned same-user channel.
//!
//! Copies are standby resources, not authority to use them. Products quiesce
//! access, validate a prepared inventory and durably commit ownership separately.
//! Sending never closes the original; errors after transmission are ambiguous.
#![doc = include_str!("../docs/live-handover.md")]

use crate::{Error, Result, endpoint, frame, net, protocol};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    fs::File,
    io,
    net::{TcpListener, TcpStream},
    time::{Duration, Instant},
};

#[cfg(unix)]
mod unix;
#[cfg(windows)]
use crate::windows::transfer as platform;
#[cfg(unix)]
use unix as platform;

/// Maximum product metadata bytes per resource.
pub const METADATA_LIMIT: usize = 64 * 1024;
/// Maximum resource copies in one finite transfer session.
pub const RESOURCE_LIMIT: u64 = 64;
const WIRE_LIMIT: usize = METADATA_LIMIT + 16 * 1024;

/// Product-verified identities expected by both ends of a fresh transfer channel.
/// These labels bind the transcript, not OS authority or durable writer rights.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Binding {
    pub transfer_id: String,
    pub predecessor: String,
    pub successor: String,
}

impl Binding {
    fn validate(&self) -> Result<()> {
        if self.predecessor == self.successor
            || [&self.transfer_id, &self.predecessor, &self.successor]
                .iter()
                .any(|id| !protocol::valid_id(id))
        {
            return Err(Error::Envelope);
        }
        Ok(())
    }
}

/// A resource whose kernel identity can be copied without restarting its work.
/// File includes pipe ends represented by `File`; sockets use their own variants.
/// Do not transfer writer leases, buffered Rust wrappers or active event-loop
/// registrations. A copy does not transfer child-parent relationships.
#[derive(Debug)]
pub enum Resource {
    File(File),
    TcpListener(TcpListener),
    TcpStream(TcpStream),
    LocalListener(net::Listener),
    LocalStream(net::Stream),
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Kind {
    File,
    TcpListener,
    TcpStream,
    LocalListener,
    LocalStream,
}

impl Resource {
    pub(crate) const fn kind(&self) -> Kind {
        match self {
            Self::File(_) => Kind::File,
            Self::TcpListener(_) => Kind::TcpListener,
            Self::TcpStream(_) => Kind::TcpStream,
            Self::LocalListener(_) => Kind::LocalListener,
            Self::LocalStream(_) => Kind::LocalStream,
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Hello {
    version: u32,
    binding: Binding,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Packet {
    sequence: u64,
    pub(crate) kind: Kind,
    metadata: Value,
    pub(crate) native: Value,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Ack {
    sequence: u64,
}

struct Session {
    stream: Option<net::Stream>,
    binding: Binding,
    timeout: Duration,
    sequence: u64,
}

impl Session {
    fn new(stream: net::Stream, binding: Binding, timeout: Duration) -> Result<Self> {
        binding.validate()?;
        if timeout.is_zero() || Instant::now().checked_add(timeout).is_none() {
            return Err(Error::Options);
        }
        endpoint::check_peer(&stream)?;
        stream.set_nonblocking(false)?;
        Ok(Self {
            stream: Some(stream),
            binding,
            timeout,
            sequence: 0,
        })
    }

    fn stream(&self) -> Result<&net::Stream> {
        self.stream.as_ref().ok_or(Error::HandoverState)
    }

    fn next(&self) -> Result<u64> {
        if self.sequence >= RESOURCE_LIMIT {
            return Err(Error::Capacity);
        }
        Ok(self.sequence + 1)
    }

    fn finish<T>(&mut self, result: Result<T>) -> Result<T> {
        if result.is_err() {
            self.stream.take();
        }
        result
    }
}

/// One sender on a fresh, dedicated transfer stream. Do not read, write or clone
/// the channel concurrently. Every transport/protocol failure poisons it.
pub struct Sender(Session);

impl Sender {
    /// Bind a same-user channel to the expected attempt and incarnations.
    /// The peer must concurrently call `Receiver::new` with the same binding.
    ///
    /// # Errors
    /// Returns invalid bindings/options, peer, framing or handshake I/O errors.
    pub fn new(stream: net::Stream, binding: Binding, timeout: Duration) -> Result<Self> {
        let session = Session::new(stream, binding, timeout)?;
        let until = Instant::now() + timeout;
        write(
            session.stream()?,
            &Hello {
                version: 1,
                binding: session.binding.clone(),
            },
            until,
        )?;
        let ack: Ack = read(session.stream()?, until)?;
        if ack.sequence != 0 {
            return Err(Error::Envelope);
        }
        Ok(Self(session))
    }

    /// Copy one resource and bounded metadata, awaiting an import acknowledgement.
    /// Success means a copy reached the receiver, not that product ownership
    /// committed. Keep the original and all holds until product reconciliation
    /// authorizes release. Neither side may use a standby copy concurrently.
    ///
    /// # Errors
    /// Returns local `FrameTooLarge`/`Capacity` before transmission. Any subsequent
    /// failure returns `OutcomeUnknown(transfer_id)` and poisons this channel;
    /// further use returns `HandoverState`. Never retry a copy automatically
    /// after an unknown result.
    pub fn send(&mut self, resource: &Resource, metadata: Value) -> Result<()> {
        self.0.stream()?;
        frame::encode_json(&metadata, METADATA_LIMIT)?;
        let sequence = self.0.next()?;
        let until = Instant::now() + self.0.timeout;
        let result = (|| {
            let stream = self.0.stream()?;
            platform::send(
                stream,
                resource,
                &mut Packet {
                    sequence,
                    kind: resource.kind(),
                    metadata,
                    native: Value::Null,
                },
                until,
            )?;
            let ack: Ack = read(stream, until)?;
            if ack.sequence != sequence {
                return Err(Error::Envelope);
            }
            Ok(())
        })();
        if result.is_err() {
            self.0.stream.take();
            return Err(Error::OutcomeUnknown(self.0.binding.transfer_id.clone()));
        }
        self.0.sequence = sequence;
        Ok(())
    }
}

/// One receiver of standby copies, with independent incarnation expectations.
pub struct Receiver(Session);

impl Receiver {
    /// Accept a fresh same-user transfer only when its binding matches exactly.
    ///
    /// # Errors
    /// Returns invalid bindings/options, peer, framing or handshake I/O errors.
    pub fn new(stream: net::Stream, binding: Binding, timeout: Duration) -> Result<Self> {
        let session = Session::new(stream, binding, timeout)?;
        let until = Instant::now() + timeout;
        let hello: Hello = read(session.stream()?, until)?;
        if hello.version != 1 || hello.binding != session.binding {
            return Err(Error::Envelope);
        }
        write(session.stream()?, &Ack { sequence: 0 }, until)?;
        Ok(Self(session))
    }

    /// Import one standby resource. Validate product metadata and retain it
    /// without I/O until ownership commits. Drop rejects/cleans the local copy.
    /// The acknowledgement proves only import; product readiness is separate.
    ///
    /// # Errors
    /// Returns `Capacity` after the session bound, or protocol/platform/I/O errors.
    /// Failed receives close imported resources and poison the channel.
    pub fn receive(&mut self) -> Result<(Resource, Value)> {
        self.0.stream()?;
        let sequence = self.0.next()?;
        let until = Instant::now() + self.0.timeout;
        let result = (|| {
            let stream = self.0.stream()?;
            let (resource, packet) = platform::receive(stream, until)?;
            if packet.sequence != sequence {
                return Err(Error::Envelope);
            }
            frame::encode_json(&packet.metadata, METADATA_LIMIT)?;
            write(stream, &Ack { sequence }, until)?;
            Ok((resource, packet.metadata))
        })();
        let result = self.0.finish(result)?;
        self.0.sequence = sequence;
        Ok(result)
    }
}

pub(crate) fn write<T: Serialize>(stream: &net::Stream, value: &T, until: Instant) -> Result<()> {
    frame::write_json(&mut Deadline { stream, until }, value, WIRE_LIMIT)
}

pub(crate) fn read<T: serde::de::DeserializeOwned>(
    stream: &net::Stream,
    until: Instant,
) -> Result<T> {
    frame::read_json(&mut Deadline { stream, until }, WIRE_LIMIT)
}

pub(crate) fn remaining(until: Instant) -> io::Result<Duration> {
    let left = until.saturating_duration_since(Instant::now());
    if left.is_zero() {
        return Err(io::ErrorKind::TimedOut.into());
    }
    Ok(left)
}

struct Deadline<'a> {
    stream: &'a net::Stream,
    until: Instant,
}
impl io::Read for Deadline<'_> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        let mut stream = self.stream;
        timed_io(
            stream,
            stream.set_read_timeout(Some(remaining(self.until)?)),
            || io::Read::read(&mut stream, buffer),
        )
    }
}
impl io::Write for Deadline<'_> {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        let mut stream = self.stream;
        timed_io(
            stream,
            stream.set_write_timeout(Some(remaining(self.until)?)),
            || io::Write::write(&mut stream, buffer),
        )
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn timed_io<T>(
    stream: &net::Stream,
    timeout: io::Result<()>,
    operation: impl FnOnce() -> io::Result<T>,
) -> io::Result<T> {
    match timeout {
        // Darwin can reject timeout changes after peer close while a final
        // acknowledgement is buffered. Try only nonblocking I/O then: ignoring
        // an EINVAL on a live stream must never remove the operation's deadline.
        Err(error) if crate::connection::peer_closed_timeout(&error) => {
            stream.set_nonblocking(true)?;
            let result = operation();
            let restored = stream.set_nonblocking(false);
            result.and_then(|value| restored.map(|()| value))
        }
        Err(error) => Err(error),
        Ok(()) => operation(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn binding() -> Binding {
        Binding {
            transfer_id: "acme-copy".into(),
            predecessor: "acme-old".into(),
            successor: "acme-new".into(),
        }
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn transfer_rejected_timeout_never_leaves_live_io_unbounded() {
        let (stream, peer) = net::Stream::pair().unwrap();
        let mut reader = &stream;
        let result = timed_io(&stream, Err(io::Error::from_raw_os_error(22)), || {
            io::Read::read(&mut reader, &mut [0])
        });
        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::WouldBlock);
        // The dedicated channel returns to blocking mode after the one attempt.
        assert!(
            !rustix::fs::fcntl_getfl(&stream)
                .unwrap()
                .contains(rustix::fs::OFlags::NONBLOCK)
        );
        drop(peer);
    }

    #[test]
    fn transfer_import_without_acknowledgement_keeps_original_and_reports_unknown() {
        let (left, right) = net::Stream::pair().unwrap();
        let imported = std::thread::spawn(move || {
            let receiver = Receiver::new(right, binding(), Duration::from_secs(2)).unwrap();
            let (copy, _) = platform::receive(
                receiver.0.stream().unwrap(),
                Instant::now() + Duration::from_secs(2),
            )
            .unwrap();
            // Deliberately lose the import acknowledgement while retaining its copy.
            drop(receiver);
            copy
        });
        let mut sender = Sender::new(left, binding(), Duration::from_secs(2)).unwrap();
        let original = Resource::File(tempfile::tempfile().unwrap());
        assert!(matches!(
            sender.send(&original, Value::Null),
            Err(Error::OutcomeUnknown(_))
        ));
        let Resource::File(copy) = imported.join().unwrap() else {
            panic!("expected file");
        };
        assert!(copy.metadata().is_ok());
        let Resource::File(original) = original else {
            unreachable!();
        };
        assert!(original.metadata().is_ok());
    }

    #[test]
    fn transfer_session_capacity_refuses_before_an_extra_copy() {
        let (left, right) = net::Stream::pair().unwrap();
        let peer = std::thread::spawn(move || {
            let mut receiver = Receiver::new(right, binding(), Duration::from_secs(2)).unwrap();
            for _ in 0..RESOURCE_LIMIT {
                drop(receiver.receive().unwrap());
            }
            assert!(matches!(receiver.receive(), Err(Error::Capacity)));
        });
        let mut sender = Sender::new(left, binding(), Duration::from_secs(2)).unwrap();
        let file = Resource::File(tempfile::tempfile().unwrap());
        for _ in 0..RESOURCE_LIMIT {
            sender.send(&file, Value::Null).unwrap();
        }
        assert!(matches!(
            sender.send(&file, Value::Null),
            Err(Error::Capacity)
        ));
        peer.join().unwrap();
    }

    #[test]
    fn transfer_replayed_sequence_is_refused_and_poisoned() {
        let (left, right) = net::Stream::pair().unwrap();
        let peer = std::thread::spawn(move || {
            let mut receiver = Receiver::new(right, binding(), Duration::from_secs(2)).unwrap();
            assert!(matches!(receiver.receive(), Err(Error::Envelope)));
            assert!(matches!(receiver.receive(), Err(Error::HandoverState)));
        });
        let sender = Sender::new(left, binding(), Duration::from_secs(2)).unwrap();
        platform::send(
            sender.0.stream().unwrap(),
            &Resource::File(tempfile::tempfile().unwrap()),
            &mut Packet {
                sequence: 0,
                kind: Kind::File,
                metadata: Value::Null,
                native: Value::Null,
            },
            Instant::now() + Duration::from_secs(2),
        )
        .unwrap();
        peer.join().unwrap();
    }
}
