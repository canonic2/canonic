//! Bounded, best-effort lifecycle records; never protocol payloads or exporters.

use crate::rpc;
use serde_json::Value;
use std::{
    sync::{
        Arc, OnceLock,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc::{self, Receiver, SyncSender},
    },
    time::Instant,
};

/// Maximum records retained by one lifecycle observation channel.
pub const MAX_OBSERVATION_BUFFER: usize = 4096;

/// A measured interval. Intervals may nest and must not be summed as exclusive work.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    /// Connection work through worker joins; excludes initial catalog preparation.
    Connection,
    /// Reader lifetime, including external input waits.
    Reader,
    /// Complete frame waiting for dispatcher processing.
    InputQueue,
    /// Strict parsing, routing, metadata and prepared input-schema validation.
    Decode,
    /// Consumer initialization, invalid-argument or admission callback.
    Callback,
    /// Lane/key queue wait, including continuation admission.
    Queue,
    /// Assignment delivery, execution, preparation and completion collection.
    Worker,
    /// Consumer handler or continuation execution, including unwinding.
    Handler,
    /// Library result checks outside encoding.
    Validation,
    /// Result construction and bounded encoding; may include schema validation.
    Encode,
    /// Legacy host-input wait; includes form publication, not only human time.
    Interaction,
    /// Encoded frame waiting for dispatcher output.
    OutputQueue,
    /// Writes, newline and flush; does not prove peer receipt.
    Publication,
    /// Cancellation and worker joins during driver cleanup.
    Drain,
}

/// Why protocol work settled; resource release may occur later.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Settlement {
    /// A reply was selected, independently of publication success.
    Reply,
    /// Notification/response processing ended without a reply.
    NoReply,
    /// Client cancellation suppresses any reply.
    Cancelled,
    /// A deadline expired.
    Deadline,
    /// Admission or workflow was refused.
    Refused,
    /// Connection closure refused the work.
    Disconnected,
    /// Work was dropped during exceptional cleanup.
    Abandoned,
}

/// A payload-free lifecycle transition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ObservationKind {
    /// Start of an inclusive interval.
    Start(Stage),
    /// End of that interval, including unsuccessful attempts.
    End(Stage),
    /// A bounded inbound protocol ID was associated after the existing parse.
    /// Responses can echo a server-issued confirmation or elicitation ID.
    ProtocolId,
    /// Frame kind and observed byte count, excluding a trailing newline.
    /// Rejected input counts are lower bounds: the reader stops at its limit.
    Frame { kind: FrameKind, bytes: usize },
    /// Logical settlement, once per observed input.
    Settled(Settlement),
    /// Last live owner released the observed input or connection.
    Released,
    /// A worker outcome was suppressed after settlement or expiry.
    Discarded,
    /// A consumer handler or continuation panicked.
    Panicked,
    /// Encoding failed or exceeded its bound; fallback may follow.
    EncodeFailed,
    /// Write/flush failed; peer receipt is unknown.
    PublicationFailed,
    /// This input answered or cancelled the identified originating input.
    Related(u64),
    /// The stream uses this input's identity until termination.
    StreamOpened,
    /// The stream ended and its observation state is released.
    StreamClosed,
    /// The input reader cannot be joined after an early driver failure.
    ReaderDetached,
}

/// Protocol frames have different lifetimes and are never all terminal replies.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameKind {
    /// Complete or refused input frame, before parsing.
    Input,
    /// Ordinary response, including errors and modern interim results.
    Reply,
    /// Server-issued legacy form or URL question.
    Interaction,
    /// Progress notification from one assignment.
    Progress,
    /// Subscription acknowledgement, change or terminal frame.
    Subscription,
    /// Cancellation of a server-issued question.
    Cancellation,
}

/// One monotonic record. Arrival order across threads is not timestamp order.
#[derive(Debug, Clone)]
pub struct Observation {
    /// Distinguishes connections sharing a producer.
    pub connection: u64,
    /// Input identity; zero identifies connection-wide work.
    pub request: u64,
    /// Scheduler assignment, distinct for each continuation round.
    pub assignment: Option<u64>,
    /// Selected lane, when admitted.
    pub lane: Option<usize>,
    /// Assigned worker index, when dispatched.
    pub worker: Option<usize>,
    /// Enclosing interval for nested worker/connection work, when applicable.
    pub parent: Option<Stage>,
    /// Optional syntactically valid inbound ID. Strings and integers remain distinct.
    /// Responses can echo a server-issued question ID.
    pub rpc_id: Option<Arc<Value>>,
    /// Timestamp on this process's monotonic clock.
    pub at: Instant,
    /// Observed boundary or outcome.
    pub kind: ObservationKind,
}

struct Shared {
    sender: SyncSender<Observation>,
    next: AtomicU64,
    dropped: AtomicU64,
}

/// Cloneable producer for [`super::Options::observations`]. No diagnostic callbacks run.
#[derive(Clone)]
pub struct ObservationSender {
    shared: Arc<Shared>,
    ids: bool,
}

/// Invalid lifecycle-channel capacity.
#[derive(Debug, thiserror::Error)]
#[error("observation capacity must be within 1..={MAX_OBSERVATION_BUFFER}")]
pub struct ObservationError;

/// Creates a bounded lifecycle channel, with protocol ID capture disabled.
/// Consumers own receiver draining, storage and any exporter threads.
///
/// # Errors
/// Returns [`ObservationError`] for zero capacity or a capacity above the limit.
pub fn observation_channel(
    capacity: usize,
) -> Result<(ObservationSender, Receiver<Observation>), ObservationError> {
    if !(1..=MAX_OBSERVATION_BUFFER).contains(&capacity) {
        return Err(ObservationError);
    }
    let (sender, receiver) = mpsc::sync_channel(capacity);
    Ok((
        ObservationSender {
            shared: Arc::new(Shared {
                sender,
                next: AtomicU64::new(1),
                dropped: AtomicU64::new(0),
            }),
            ids: false,
        },
        receiver,
    ))
}

impl ObservationSender {
    /// Enables bounded inbound protocol IDs on this producer clone.
    /// Response IDs can echo server-issued questions.
    #[must_use]
    pub fn with_rpc_ids(mut self, enabled: bool) -> Self {
        self.ids = enabled;
        self
    }

    /// Saturating count of records lost to full/closed receivers or exhausted IDs.
    /// A nonzero count makes evidence incomplete, even if no loss record arrived.
    #[must_use]
    pub fn dropped(&self) -> u64 {
        self.shared.dropped.load(Ordering::Relaxed)
    }

    fn lost(&self) {
        let _ = self
            .shared
            .dropped
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |value| {
                Some(value.saturating_add(1))
            });
    }

    fn next(&self) -> Option<u64> {
        self.shared
            .next
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |value| {
                value.checked_add(1)
            })
            .map_err(|_| self.lost())
            .ok()
    }
}

struct Live {
    sender: ObservationSender,
    connection: u64,
    request: u64,
    id: OnceLock<Arc<Value>>,
    settled: AtomicBool,
}

/// State follows live work rather than a completed-request registry.
#[derive(Clone, Default)]
pub(super) struct Trace {
    live: Option<Arc<Live>>,
    assignment: Option<u64>,
    lane: Option<usize>,
    worker: Option<usize>,
    parent: Option<Stage>,
}

impl Trace {
    pub(super) fn connection(sender: Option<&ObservationSender>) -> Self {
        let Some(sender) = sender else {
            return Self::default();
        };
        let Some(connection) = sender.next() else {
            return Self::default();
        };
        Self {
            live: Some(Arc::new(Live {
                sender: sender.clone(),
                connection,
                request: 0,
                id: OnceLock::new(),
                settled: AtomicBool::new(true),
            })),
            ..Self::default()
        }
    }

    pub(super) fn input(&self) -> Self {
        let Some(live) = &self.live else {
            return Self::default();
        };
        let Some(request) = live.sender.next() else {
            return Self::default();
        };
        Self {
            live: Some(Arc::new(Live {
                sender: live.sender.clone(),
                connection: live.connection,
                request,
                id: OnceLock::new(),
                settled: AtomicBool::new(false),
            })),
            ..Self::default()
        }
    }

    pub(super) fn enabled(&self) -> bool {
        self.live.is_some()
    }

    pub(super) fn bind(&self, value: &Value) {
        if let Some(live) = &self.live
            && live.sender.ids
            && let Some(id) = value.get("id").filter(|id| rpc::valid_id(id))
        {
            let _ = live.id.set(Arc::new(id.clone()));
            self.event(ObservationKind::ProtocolId);
        }
    }

    pub(super) fn lane(&mut self, lane: usize) {
        self.lane = Some(lane);
    }

    pub(super) fn assigned(&self, assignment: u64, worker: usize) -> Self {
        Self {
            assignment: Some(assignment),
            worker: Some(worker),
            ..self.clone()
        }
    }

    pub(super) fn relate(&self, origin: &Self) {
        if let Some(live) = &origin.live {
            self.event(ObservationKind::Related(live.request));
        }
    }

    pub(super) fn event(&self, kind: ObservationKind) {
        if let Some(live) = &self.live {
            live.emit(self, kind);
        }
    }

    pub(super) fn settle(&self, why: Settlement) {
        if let Some(live) = &self.live
            && !live.settled.swap(true, Ordering::Relaxed)
        {
            self.event(ObservationKind::Settled(why));
        }
    }

    pub(super) fn span(&self, stage: Stage) -> Span {
        self.event(ObservationKind::Start(stage));
        Span {
            trace: self.clone(),
            stage,
        }
    }

    pub(super) fn span_with_parent(&self, stage: Stage, parent: Stage) -> Span {
        let mut trace = self.clone();
        trace.parent = Some(parent);
        trace.span(stage)
    }
}

impl Live {
    fn emit(&self, trace: &Trace, kind: ObservationKind) {
        let event = Observation {
            connection: self.connection,
            request: self.request,
            assignment: trace.assignment,
            lane: trace.lane,
            worker: trace.worker,
            parent: trace.parent,
            rpc_id: self.id.get().cloned(),
            at: Instant::now(),
            kind,
        };
        if self.sender.shared.sender.try_send(event).is_err() {
            self.sender.lost();
        }
    }
}

impl Drop for Live {
    fn drop(&mut self) {
        if !self.settled.load(Ordering::Relaxed) {
            self.emit(
                &Trace::default(),
                ObservationKind::Settled(Settlement::Abandoned),
            );
        }
        self.emit(&Trace::default(), ObservationKind::Released);
    }
}

pub(super) struct Span {
    pub(super) trace: Trace,
    stage: Stage,
}

impl Drop for Span {
    fn drop(&mut self) {
        self.trace.event(ObservationKind::End(self.stage));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn observation_identity_exhaustion_refuses_to_wrap_and_loss_saturates() {
        let (sender, receiver) = observation_channel(1).unwrap();
        sender.shared.next.store(u64::MAX, Ordering::Relaxed);
        assert!(!Trace::connection(Some(&sender)).enabled());
        assert_eq!(sender.dropped(), 1);
        assert_eq!(sender.shared.next.load(Ordering::Relaxed), u64::MAX);
        sender.shared.dropped.store(u64::MAX, Ordering::Relaxed);
        sender.lost();
        assert_eq!(sender.dropped(), u64::MAX);
        assert_eq!(receiver.try_iter().count(), 0);
    }
}
