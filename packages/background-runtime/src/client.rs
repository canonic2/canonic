//! Non-launching inspection, explicit connect-or-start and one-shot requests.

use crate::{
    Error, Result,
    endpoint::{self, Root},
    frame,
    launch::Launcher,
    net::Stream,
    protocol::{Code, Control, Identity, Reply, Request},
    version::Version,
};
use std::time::{Duration, Instant};

/// Readiness and per-frame deadlines; polling uses capped exponential backoff.
pub struct Options {
    /// Overall probe budget, shared by connection, writes and both reply frames.
    pub readiness: Duration,
    /// Per-frame read/write budget, capped by remaining readiness time for probes.
    pub io: Duration,
    pub frame_limit: usize,
}
impl Default for Options {
    fn default() -> Self {
        Self {
            readiness: Duration::from_secs(15),
            io: Duration::from_secs(3),
            frame_limit: frame::DEFAULT_LIMIT,
        }
    }
}

/// Result of an explicit upgrade probe. Busy managers are never killed or drained implicitly.
#[derive(Debug)]
pub enum Upgrade {
    /// Admission was atomically closed on the targeted empty incarnation.
    Retired,
    /// Live obligations prevented replacement. Compatible work may continue.
    Busy {
        identity: Identity,
        compatible: bool,
        blockers: serde_json::Value,
    },
}

/// Ask an older manager to retire after the product decides an upgrade is wanted.
/// The retirement request targets the observed incarnation; a busy compatible
/// manager remains usable, and an incompatible one keeps its control surface.
///
/// # Errors
/// Returns `StaleManager` if the observed incarnation was replaced before the
/// retirement request, or non-launching control, transport and malformed-reply
/// errors. A stale refusal leaves the replacement untouched.
pub fn upgrade(
    root: &Root,
    protocol: u32,
    requires: &[String],
    options: &Options,
) -> Result<Upgrade> {
    let identity = connect(root, None, options)?;
    let reply = control(
        root,
        &Control {
            control_version: 1,
            request_id: "upgrade-retire".into(),
            action: "retire_if_empty".into(),
            requires: vec![],
            manager_id: Some(identity.manager_id.clone()),
            arguments: serde_json::Value::Null,
        },
        options,
    )?;
    if reply.code == Some(Code::StaleManager) {
        return Err(Error::StaleManager);
    }
    match reply
        .data
        .get("retired")
        .and_then(serde_json::Value::as_bool)
    {
        Some(true) => Ok(Upgrade::Retired),
        Some(false) => Ok(Upgrade::Busy {
            compatible: identity.require(protocol, requires).is_ok(),
            identity,
            blockers: reply.data["blockers"].clone(),
        }),
        None => Err(Error::Envelope),
    }
}

/// Connect or explicitly start. This sends no application request.
/// Each readiness wait activates its launcher at most once; a child that has
/// not acquired its lease yet is still starting, not evidence for another launch.
///
/// # Errors
/// Returns `Absent` without a launcher, `Starting` at the deadline, `Draining`
/// during inspection, or
/// identity/transport errors or launch failure. Successful readiness requires
/// a control identity reply, not merely an existing socket pathname.
pub fn connect(
    root: &Root,
    launcher: Option<&dyn Launcher>,
    options: &Options,
) -> Result<Identity> {
    connect_until(root, launcher, options, Instant::now() + options.readiness)
}

/// Connect using a release version, retiring an older manager without killing work.
/// An empty older manager exits immediately and the supplied launcher starts a
/// replacement after lease release. A busy one keeps serving and remembers to
/// retire once its work and holds settle. Check the returned identity's ordinary
/// protocol and capabilities before sending product work.
///
/// This explicitly permits startup and retirement. Read-only inspection uses
/// [`connect`] without a launcher instead.
///
/// # Errors
/// Returns `InvalidVersion` for unorderable release labels, connection/control
/// failures or `Starting` at the shared readiness deadline. A launcher that
/// starts another older manager returns `Incompatible`, without restart churn.
pub fn connect_versioned(
    root: &Root,
    launcher: &dyn Launcher,
    build: &str,
    options: &Options,
) -> Result<Identity> {
    let candidate = Version::parse(build)?;
    let deadline = Instant::now() + options.readiness;
    let mut retirement_target = None;
    let mut delay = Duration::from_millis(2);
    loop {
        let result = connect_until(root, Some(launcher), options, deadline);
        let identity = match result {
            Err(Error::Draining) => {
                pause_replacement(&mut delay, deadline)?;
                continue;
            }
            Err(Error::OutcomeUnknown(id)) if id == "readiness" => {
                pause_replacement(&mut delay, deadline)?;
                continue;
            }
            other => other?,
        };
        if !candidate.is_newer_than(&Version::parse(&identity.build)?) {
            return Ok(identity);
        }
        if retirement_target
            .as_ref()
            .is_some_and(|target| target != &identity.manager_id)
        {
            return Err(Error::Incompatible);
        }
        retirement_target = Some(identity.manager_id.clone());
        let reply = control_until(
            root,
            &version_retirement_request(&identity, build),
            options,
            Some(deadline),
        );
        let reply = match reply {
            Err(Error::Absent | Error::Starting | Error::Draining | Error::Capacity) => {
                pause_replacement(&mut delay, deadline)?;
                continue;
            }
            Err(Error::OutcomeUnknown(id)) if id == "version-retire" => {
                // Version retirement is idempotent and incarnation-checked.
                // Reobserve identity before retrying; product work is never sent.
                pause_replacement(&mut delay, deadline)?;
                continue;
            }
            other => other?,
        };
        if reply.code == Some(Code::StaleManager) {
            pause_replacement(&mut delay, deadline)?;
            continue;
        }
        match reply
            .data
            .get("retired")
            .and_then(serde_json::Value::as_bool)
        {
            Some(false) => return Ok(identity),
            Some(true) => {}
            None => return Err(Error::Envelope),
        }
        pause_replacement(&mut delay, deadline)?;
    }
}

fn version_retirement_request(identity: &Identity, build: &str) -> Control {
    let supported = identity
        .capabilities
        .iter()
        .any(|c| c == crate::protocol::VERSION_RETIREMENT);
    Control {
        control_version: 1,
        request_id: "version-retire".into(),
        action: if supported {
            "retire_if_older"
        } else {
            "retire_if_empty"
        }
        .into(),
        requires: if supported {
            vec![crate::protocol::VERSION_RETIREMENT.into()]
        } else {
            vec![]
        },
        manager_id: Some(identity.manager_id.clone()),
        arguments: if supported {
            serde_json::json!({"version":build})
        } else {
            serde_json::Value::Null
        },
    }
}

fn pause_replacement(delay: &mut Duration, deadline: Instant) -> Result<()> {
    let remaining = deadline.saturating_duration_since(Instant::now());
    if remaining.is_zero() {
        return Err(Error::Starting);
    }
    std::thread::sleep((*delay).min(remaining));
    *delay = (*delay * 2).min(Duration::from_millis(100));
    Ok(())
}

pub(crate) fn connect_until(
    root: &Root,
    launcher: Option<&dyn Launcher>,
    options: &Options,
    deadline: Instant,
) -> Result<Identity> {
    let mut delay = Duration::from_millis(2);
    let mut started = false;
    loop {
        if Instant::now() >= deadline {
            return Err(Error::Starting);
        }
        let result = control_until(
            root,
            &Control {
                control_version: 1,
                request_id: "readiness".into(),
                action: "identity".into(),
                requires: vec![],
                manager_id: None,
                arguments: serde_json::Value::Null,
            },
            options,
            Some(deadline),
        );
        if Instant::now() >= deadline {
            return Err(Error::Starting);
        }
        match result {
            Ok(reply) => {
                if matches!(
                    reply.data["lifecycle"].as_str(),
                    Some("draining" | "exiting")
                ) {
                    if launcher.is_none() {
                        return Err(Error::Draining);
                    }
                    pause_replacement(&mut delay, deadline)?;
                    continue;
                }
                return serde_json::from_value(reply.data).map_err(|_| Error::Envelope);
            }
            Err(Error::Absent | Error::Starting) => {
                let Some(launcher) = launcher else {
                    return result.map(|_| unreachable!());
                };
                if Instant::now() >= deadline {
                    return Err(Error::Starting);
                }
                // Wait for any existing holder to finish before activating.
                // After activation, an unheld lease may mean the child is still starting.
                if !started && !root.leased()? {
                    launcher.start()?;
                    started = true;
                }
                std::thread::sleep(delay.min(deadline.saturating_duration_since(Instant::now())));
                delay = (delay * 2).min(Duration::from_millis(100));
            }
            Err(Error::Capacity) if Instant::now() < deadline => {
                std::thread::sleep(delay.min(deadline.saturating_duration_since(Instant::now())));
                delay = (delay * 2).min(Duration::from_millis(100));
            }
            Err(Error::Draining) if launcher.is_some() => {
                pause_replacement(&mut delay, deadline)?;
            }
            Err(Error::OutcomeUnknown(id)) if launcher.is_some() && id == "readiness" => {
                pause_replacement(&mut delay, deadline)?;
            }
            Err(error) => return Err(error),
        }
    }
}

/// Issue a control without ever starting a manager.
///
/// # Errors
/// Returns absence, startup, transport, malformed-envelope or control refusal errors.
pub fn control(root: &Root, request: &Control, options: &Options) -> Result<Reply> {
    control_until(root, request, options, None)
}

fn control_until(
    root: &Root,
    request: &Control,
    options: &Options,
    deadline: Option<Instant>,
) -> Result<Reply> {
    let timeout = if let Some(until) = deadline {
        let remaining = until.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(Error::Starting);
        }
        options.io.min(remaining)
    } else {
        options.io
    };
    let mut stream = endpoint::connect(root, true, timeout)?;
    exchange(&mut stream, request, &request.request_id, options, deadline)
}

/// Send one product request exactly once, preserving its stable ID on ambiguity.
///
/// # Errors
/// Returns definite refusal codes or `OutcomeUnknown` after any partial send or
/// lost acknowledgement/result. Failure to receive admission alone is NOT proof
/// that the server never ran the request: its acknowledgement may have been lost.
pub fn call(root: &Root, request: &Request, options: &Options) -> Result<Reply> {
    call_until(root, request, options, None)
}

/// Complete a handoff handshake and return its reply plus a checked open stream.
/// The stream is blocking with handshake timeouts cleared. Product code owns
/// subsequent framing, deadlines and closure; this function never replays work.
/// No product bytes are read beyond the two handshake frames.
///
/// # Errors
/// Returns connection, capability, capacity and transport errors, `OutcomeUnknown`
/// on an ambiguous handshake, or `HandoffRefused` with the manager's error reply.
pub fn open(root: &Root, request: &Request, options: &Options) -> Result<(Reply, Stream)> {
    let mut request = request.clone();
    if !request
        .requires
        .iter()
        .any(|c| c == crate::protocol::CONNECTION_HANDOFF)
    {
        request
            .requires
            .push(crate::protocol::CONNECTION_HANDOFF.into());
    }
    let id = request.request_id.clone();
    let mut stream = endpoint::connect(root, false, options.io)?;
    let reply = exchange(
        &mut stream,
        &crate::protocol::ConnectionRequest {
            connection: request,
        },
        &id,
        options,
        None,
    )?;
    if reply.code.is_some() || reply.accepted.is_some() {
        return Err(Error::HandoffRefused(Box::new(reply)));
    }
    crate::connection::clear_deadlines(&stream)?;
    Ok((reply, stream))
}

pub(crate) fn call_until(
    root: &Root,
    request: &Request,
    options: &Options,
    deadline: Option<Instant>,
) -> Result<Reply> {
    let remaining = deadline.map_or(options.io, |until| {
        options
            .io
            .min(until.saturating_duration_since(Instant::now()))
    });
    if remaining.is_zero() {
        return Err(Error::Starting);
    }
    let mut stream = endpoint::connect(root, false, remaining)?;
    exchange(&mut stream, request, &request.request_id, options, deadline)
}

fn exchange(
    stream: &mut Stream,
    request: &impl serde::Serialize,
    id: &str,
    options: &Options,
    deadline: Option<Instant>,
) -> Result<Reply> {
    // Validate size before sending anything, so local size errors are definite.
    let bytes = frame::encode_json(request, options.frame_limit)?;
    if bytes.len() > options.frame_limit {
        return Err(Error::FrameTooLarge);
    }
    let sent = frame::write(
        &mut FrameDeadline {
            stream,
            until: frame_until(options.io, deadline),
        },
        &bytes,
        options.frame_limit,
    );
    let read = |stream: &mut Stream| -> Result<Reply> {
        let mut input = FrameDeadline {
            stream,
            until: frame_until(options.io, deadline),
        };
        let reply: Reply = frame::read_json(&mut input, options.frame_limit)
            .map_err(|_| Error::OutcomeUnknown(id.into()))?;
        if reply.control_version != 1 || (!reply.request_id.is_empty() && reply.request_id != id) {
            return Err(Error::Envelope);
        }
        Ok(reply)
    };
    let ack = read(stream)?;
    if ack.accepted == Some(false) {
        return refusal(ack);
    }
    if sent.is_err() {
        return Err(Error::OutcomeUnknown(id.into()));
    }
    if ack.accepted != Some(true) {
        return Err(Error::Envelope);
    }
    let reply = read(stream)?;
    if reply.manager_id != ack.manager_id || reply.request_id != id || reply.accepted.is_some() {
        return Err(Error::Envelope);
    }
    Ok(reply)
}

fn frame_until(io: Duration, deadline: Option<Instant>) -> Instant {
    let until = Instant::now() + io;
    deadline.map_or(until, |deadline| until.min(deadline))
}

struct FrameDeadline<'a> {
    stream: &'a Stream,
    until: Instant,
}

/// Darwin can reject a timeout change after the peer exits.
fn darwin_invalid_timeout(error: &std::io::Error) -> bool {
    #[cfg(target_os = "macos")]
    {
        error.raw_os_error() == Some(nix::errno::Errno::EINVAL as i32)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = error;
        false
    }
}
impl std::io::Read for FrameDeadline<'_> {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        let remaining = self.until.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(std::io::ErrorKind::TimedOut.into());
        }
        match self.stream.set_read_timeout(Some(remaining)) {
            Ok(()) => {}
            Err(error) if darwin_invalid_timeout(&error) => {
                // Darwin can reject timeout changes after peer exit while a
                // complete reply is buffered. Nonblocking reads preserve that
                // reply without letting an incomplete frame exceed its deadline.
                self.stream.set_nonblocking(true)?;
            }
            Err(error) => return Err(error),
        }
        self.stream.read(buffer)
    }
}

impl std::io::Write for FrameDeadline<'_> {
    fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
        let remaining = self.until.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(std::io::ErrorKind::TimedOut.into());
        }
        self.stream.set_write_timeout(Some(remaining))?;
        self.stream.write(buffer)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn refusal(reply: Reply) -> Result<Reply> {
    match reply.code {
        Some(Code::Draining) => Err(Error::Draining),
        Some(Code::Starting) => Err(Error::Starting),
        Some(Code::Incompatible) => Err(Error::Incompatible),
        Some(Code::Capacity) => Err(Error::Capacity),
        Some(Code::InvalidVersion) => Err(Error::InvalidVersion),
        _ => Ok(reply),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_deadline_preserves_buffered_reply_after_peer_exit() {
        let (stream, mut peer) = Stream::pair().unwrap();
        frame::write_json(&mut peer, &"acme", frame::DEFAULT_LIMIT).unwrap();
        drop(peer);
        let mut input = FrameDeadline {
            stream: &stream,
            until: Instant::now() + Duration::from_secs(1),
        };
        let reply: String = frame::read_json(&mut input, frame::DEFAULT_LIMIT).unwrap();
        assert_eq!(reply, "acme");
    }
}
