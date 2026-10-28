//! Bounded request recovery with explicit product authority for ambiguous replay.

use crate::{
    Error, Result, client,
    endpoint::Root,
    launch::Launcher,
    protocol::{Identity, Reply, Request},
};
use std::time::{Duration, Instant};

/// Product decision after reconnection and durable outcome reconciliation.
pub enum Decision {
    /// The product recovered the final result from its durable records.
    Recovered(Reply),
    /// Replay the identical envelope. The product guarantees durable ID/content
    /// deduplication or has established that the original attempt had no effects.
    Replay,
    /// The product cannot establish a safe result or safe replay.
    Unknown,
}

/// Bounds for one recovery operation, including reconnects and application sends.
pub struct Options {
    /// Maximum application sends, including the first attempt. Must be positive.
    pub attempts: usize,
    /// Overall monotonic budget, capped across every connect and frame operation.
    /// Product reconciliation and launcher callbacks own their execution time.
    pub timeout: Duration,
}

/// Connect, send, and recover across manager failure using a product decision.
/// The same request ID and payload are used for every authorized replay.
/// `reconcile` runs only after an ambiguous send and successful reconnection;
/// the replacement has initialized its durable state before the callback runs.
/// No ambiguous request is replayed without `Decision::Replay`.
///
/// # Errors
/// Returns `Options` for zero bounds; ordinary connection/refusal errors before
/// ambiguity; `OutcomeUnknown` if recovery or its budget cannot resolve a sent
/// request; `Envelope` for an invalid recovered result; or a callback error.
pub fn call(
    root: &Root,
    launcher: &dyn Launcher,
    request: &Request,
    transport: &client::Options,
    options: &Options,
    mut reconcile: impl FnMut(&Request, &Identity) -> Result<Decision>,
) -> Result<Reply> {
    if options.attempts == 0 || options.timeout.is_zero() {
        return Err(Error::Options);
    }
    let deadline = Instant::now() + options.timeout;
    let mut ambiguous = false;
    let mut delay = Duration::from_millis(2);
    for attempt in 0..=options.attempts {
        let identity = client::connect_until(root, Some(launcher), transport, deadline)
            .map_err(|error| uncertain(error, ambiguous, request))?;
        identity
            .require(request.protocol, &request.requires)
            .map_err(|error| uncertain(error, ambiguous, request))?;
        if ambiguous {
            match reconcile(request, &identity)? {
                Decision::Recovered(reply) => return recovered(reply, request),
                Decision::Unknown => return Err(Error::OutcomeUnknown(request.request_id.clone())),
                Decision::Replay => {}
            }
        }
        if attempt == options.attempts || Instant::now() >= deadline {
            return Err(uncertain(Error::Starting, ambiguous, request));
        }
        match client::call_until(root, request, transport, Some(deadline)) {
            Err(Error::OutcomeUnknown(_)) => ambiguous = true,
            Err(Error::Absent | Error::Starting | Error::Draining | Error::Capacity) => {}
            Ok(reply) => return Ok(reply),
            Err(error) => return Err(uncertain(error, ambiguous, request)),
        }
        std::thread::sleep(delay.min(deadline.saturating_duration_since(Instant::now())));
        delay = (delay * 2).min(Duration::from_millis(100));
    }
    Err(uncertain(Error::Starting, ambiguous, request))
}

fn uncertain(error: Error, ambiguous: bool, request: &Request) -> Error {
    if ambiguous {
        Error::OutcomeUnknown(request.request_id.clone())
    } else {
        error
    }
}

fn recovered(reply: Reply, request: &Request) -> Result<Reply> {
    if reply.control_version != 1
        || reply.request_id != request.request_id
        || reply.accepted.is_some()
    {
        return Err(Error::Envelope);
    }
    Ok(reply)
}
