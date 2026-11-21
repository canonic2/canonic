//! Between-call worker replacement behind a consumer-owned stable frontend.
//!
//! The supervisor owns dispatch reservations, one candidate, irreversible
//! cutover and bounded attempt outcomes. Adapters own process/control I/O and
//! protocol state; products own executable authorization and durable commit.
#![doc = include_str!("../docs/worker-replacement.md")]

use crate::{
    Error, Result,
    core::{self, Admission, Permit},
    endpoint::Lease,
    protocol::{self, Identity},
    version::Version,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    sync::{
        Arc, Condvar, Mutex, MutexGuard,
        atomic::{AtomicBool, Ordering},
    },
    task::{Wake, Waker},
    time::{Duration, Instant},
};

mod transition;

/// Explicit bounds. Protocol adapters separately bound queued requests and bytes.
#[derive(Clone, Copy, Debug)]
pub struct Options {
    pub preparation: Duration,
    pub activation: Duration,
    pub cleanup: Duration,
    /// Maximum concurrent dispatch reservations, including undelivered replies.
    pub capacity: usize,
    /// Maximum current plus retained attempts; unresolved work is never evicted.
    pub history: usize,
    /// Encoded outcome budget, including a reserved current-attempt slot.
    pub history_bytes: usize,
}
impl Options {
    fn validate(self) -> Result<()> {
        if [self.preparation, self.activation, self.cleanup]
            .iter()
            .any(|time| time.is_zero() || Instant::now().checked_add(*time).is_none())
            || self.capacity == 0
            || self.capacity > 65_536
            || self.history == 0
            || self.history > 65_536
            || self.history_bytes < OUTCOME_LIMIT
            || self.history_bytes > 64 * 1024 * 1024
        {
            return Err(Error::Options);
        }
        Ok(())
    }
}

/// Product-authorized candidate selection. `key` binds exact installation inputs.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Selection {
    pub key: String,
    pub build: String,
    pub protocol: u32,
    pub requires: Vec<String>,
}

/// A monotonically numbered request scoped to this frontend incarnation.
/// Reusing a retained number requires identical inputs. An older, evicted number
/// refuses instead of becoming a fresh attempt. External IDs are mapped by the adapter.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub frontend: String,
    pub sequence: u64,
    pub selection: Selection,
}

/// Payload-free replacement phase. Only `Serving`, `Replaced` and `Deferred`
/// admit new dispatch. `Reconcile` retains both process obligations.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    Serving,
    Preparing,
    Fenced,
    Committed,
    Activating,
    Replaced,
    Deferred,
    Reconcile,
    Stopped,
}

/// Current or retained outcome; errors are returned by the initiating call.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Commit {
    NotStarted,
    Unknown,
    Confirmed,
}

/// Attempt progress; cleanup never erases whether commit may have occurred.
#[derive(Clone, Debug, Serialize)]
pub struct Outcome {
    pub request: Request,
    pub phase: Phase,
    pub successor: Option<Identity>,
    pub fenced: bool,
    pub commit: Commit,
}

/// Coherent, bounded status without product payloads or process I/O.
#[derive(Clone, Debug, Serialize)]
pub struct Status {
    pub frontend: Identity,
    pub active: Option<Identity>,
    pub phase: Phase,
    pub closing: bool,
    pub work: usize,
    pub held: usize,
    pub attempt: Option<Outcome>,
}

/// Exact identities and one shared absolute phase deadline for adapter callbacks.
pub struct Transition<'a> {
    pub request: &'a Request,
    pub predecessor: &'a Identity,
    pub successor: &'a Identity,
    pub deadline: Instant,
}

/// Process/resource adapter. Methods run without supervisor/admission locks.
/// Implementations must bound I/O by the supplied deadline and retain owned
/// process handles on errors. A worker must independently observe frontend death
/// through a private owner channel or equivalent native ownership, not just Drop.
/// Identity is immutable and must describe the actual authenticated process.
pub trait Worker: Send + Sync + 'static {
    fn identity(&self) -> &Identity;
    /// Positively observe exit of this exact worker to permit same-build recovery.
    ///
    /// # Errors
    /// An observation failure leaves exit unproved.
    fn exited(&self) -> Result<bool> {
        Ok(false)
    }
    /// Idempotently validate inert standby state and compatibility without effects.
    /// Called before waiting for existing work and again against fenced state.
    /// # Errors
    /// Returns compatibility, deadline or I/O failures.
    fn prepare(&self, transition: &Transition<'_>) -> Result<()>;
    /// Fence all predecessor writers/effects, retaining exclusion for commit.
    /// # Errors
    /// Any failure requires reconciliation; old dispatch will never reopen.
    fn fence(&self, transition: &Transition<'_>) -> Result<()>;
    /// Disarm transferred cleanup, release writer locks and reap the predecessor.
    /// Must be idempotent; success proves that this process has retired.
    /// # Errors
    /// Returns uncertainty without abandoning process/resource ownership.
    fn retire(&self, transition: &Transition<'_>) -> Result<()>;
    /// Acquire product writer authority and restore obligations before readiness.
    /// # Errors
    /// Returns activation failures; the supervisor retains the candidate.
    fn activate(&self, transition: &Transition<'_>) -> Result<()>;
    /// Apply explicit shutdown/candidate-abort policy and reap this exact process.
    /// Must be idempotent, including after retirement. Success proves cleanup.
    /// # Errors
    /// Returns unresolved cleanup; the handle remains retained for another shutdown.
    fn stop(&self, deadline: Instant) -> Result<()>;
}

struct Slot<W> {
    worker: Arc<W>,
    identity: Option<Identity>,
    gate: Admission,
    lease: Arc<Lease>,
}
impl<W> Clone for Slot<W> {
    fn clone(&self) -> Self {
        Self {
            worker: Arc::clone(&self.worker),
            identity: self.identity.clone(),
            gate: self.gate.clone(),
            lease: Arc::clone(&self.lease),
        }
    }
}
impl<W> Slot<W> {
    fn identity(&self) -> &Identity {
        self.identity
            .as_ref()
            .expect("active worker identity was validated")
    }
}
struct State<W> {
    active: Option<Slot<W>>,
    candidate: Option<Slot<W>>,
    phase: Phase,
    current: Option<Outcome>,
    history: BTreeMap<u64, (Outcome, usize)>,
    history_bytes: usize,
    high_water: u64,
}

#[derive(Default)]
struct Notification {
    epoch: Mutex<u64>,
    changed: Condvar,
}
impl Notification {
    fn notify(&self) {
        let mut epoch = self
            .epoch
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        *epoch = epoch.wrapping_add(1);
        self.changed.notify_all();
    }
}
impl Wake for Notification {
    fn wake(self: Arc<Self>) {
        self.notify();
    }
    fn wake_by_ref(self: &Arc<Self>) {
        self.notify();
    }
}

/// One controller owns the domain lease through all replacements and shutdown.
/// It never owns or rewrites host transport bytes. The active worker passed to
/// `new` is already authenticated and serving; candidates come from `replace`.
pub struct Supervisor<W: Worker> {
    lease: Arc<Lease>,
    frontend: Identity,
    options: Options,
    state: Mutex<State<W>>,
    operation: Mutex<()>,
    closing: AtomicBool,
    notification: Arc<Notification>,
}

/// Retain this reservation through dispatch AND final reply ownership. The
/// worker remains the original target during preparation; cancellation can use it.
#[must_use]
pub struct Dispatch<W: Worker> {
    slot: Slot<W>,
    permit: Permit,
}
impl<W: Worker> Dispatch<W> {
    #[must_use]
    pub fn worker(&self) -> &W {
        &self.slot.worker
    }
    /// Pin a resource that outlives the call. Initial between-call replacement
    /// defers until it settles; a copy of a handle cannot erase this hold.
    /// # Errors
    /// Returns admission errors without releasing the reservation.
    pub fn hold(&self) -> Result<Hold<W>> {
        Ok(Hold {
            hold: self.permit.hold()?,
            slot: self.slot.clone(),
        })
    }
}

/// A resource obligation retaining its original worker and domain exclusion,
/// even after its dispatch reservation and supervisor are dropped.
#[must_use]
pub struct Hold<W: Worker> {
    // Release accounting before releasing the worker and lease.
    hold: core::Hold,
    slot: Slot<W>,
}
impl<W: Worker> Hold<W> {
    /// Pin another obligation with the same worker and domain lease, including
    /// while admission is paused or closed.
    pub fn split(&self) -> Self {
        Self {
            hold: self.hold.split(),
            slot: self.slot.clone(),
        }
    }
}

fn lock<T>(mutex: &Mutex<T>) -> Result<MutexGuard<'_, T>> {
    mutex.lock().map_err(|_| Error::Worker)
}
fn identity(identity: &Identity) -> Result<()> {
    if !protocol::valid_id(&identity.manager_id)
        || identity.control_version != protocol::CONTROL_VERSION
        || identity.build.len() > 128
        || identity.capabilities.len() > 128
        || identity
            .capabilities
            .iter()
            .any(|name| !protocol::valid_id(name))
    {
        return Err(Error::Options);
    }
    Ok(())
}

impl<W: Worker> Supervisor<W> {
    /// Retain kernel exclusion and install an already active worker.
    /// # Errors
    /// Returns invalid bounds/identities or non-semantic release labels.
    pub fn new(lease: Lease, frontend: Identity, active: W, options: Options) -> Result<Self> {
        options.validate()?;
        identity(&frontend)?;
        let active_identity = transition::callback(|| {
            let observed = active.identity();
            identity(observed)?;
            Ok(observed.clone())
        })?;
        Version::parse(&active_identity.build)?;
        if frontend.manager_id == active_identity.manager_id {
            return Err(Error::Options);
        }
        let notification = Arc::new(Notification::default());
        let lease = Arc::new(lease);
        let gate = Admission::new();
        gate.set_waker(Some(Waker::from(Arc::clone(&notification))));
        gate.ready();
        Ok(Self {
            lease: Arc::clone(&lease),
            frontend,
            options,
            state: Mutex::new(State {
                active: Some(Slot {
                    worker: Arc::new(active),
                    identity: Some(active_identity),
                    gate,
                    lease,
                }),
                candidate: None,
                phase: Phase::Serving,
                current: None,
                history: BTreeMap::new(),
                history_bytes: 0,
                high_water: 0,
            }),
            operation: Mutex::new(()),
            closing: AtomicBool::new(false),
            notification,
        })
    }
    #[must_use]
    pub fn lease(&self) -> &Lease {
        &self.lease
    }
    /// Reserve the current target without waiting or buffering a request.
    /// # Errors
    /// Returns `Draining` while paused/closed and `Capacity` at the dispatch cap.
    pub fn enter(&self) -> Result<Dispatch<W>> {
        let state = lock(&self.state)?;
        if self.closing.load(Ordering::SeqCst)
            || !matches!(
                state.phase,
                Phase::Serving | Phase::Replaced | Phase::Deferred
            )
        {
            return Err(Error::Draining);
        }
        let slot = state.active.as_ref().ok_or(Error::Draining)?;
        if slot.gate.snapshot().work >= self.options.capacity {
            return Err(Error::Capacity);
        }
        Ok(Dispatch {
            slot: slot.clone(),
            permit: slot.gate.enter(false)?,
        })
    }
    /// Latch host disconnect/shutdown immediately, including during a callback.
    /// Call `shutdown` to drive cleanup; a running replacement also observes it.
    pub fn close(&self) {
        // Serialize the latch with final dispatch publication. No adapter I/O
        // runs under this short lock.
        let state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        self.closing.store(true, Ordering::SeqCst);
        drop(state);
        self.notification.notify();
    }
    /// Inspect status without launching, invoking adapters or renewing timeouts.
    /// # Errors
    /// Returns a poisoned supervisor error.
    pub fn status(&self) -> Result<Status> {
        let state = lock(&self.state)?;
        let counts = state.active.as_ref().map(|slot| slot.gate.snapshot());
        Ok(Status {
            frontend: self.frontend.clone(),
            active: state.active.as_ref().map(|slot| slot.identity().clone()),
            phase: state.phase,
            closing: self.closing.load(Ordering::SeqCst),
            work: counts.map_or(0, |snapshot| snapshot.work),
            held: counts.map_or(0, |snapshot| snapshot.held),
            attempt: state.current.clone(),
        })
    }
    /// Read a retained exact attempt. Unknown/evicted numbers never start work.
    /// # Errors
    /// Returns `HandoverState` for missing, foreign or changed requests.
    pub fn outcome(&self, request: &Request) -> Result<Outcome> {
        let state = lock(&self.state)?;
        Self::find(&state, request)?.ok_or(Error::HandoverState)
    }
    fn find(state: &State<W>, request: &Request) -> Result<Option<Outcome>> {
        let outcome = state
            .current
            .as_ref()
            .filter(|entry| entry.request.sequence == request.sequence)
            .or_else(|| {
                state
                    .history
                    .get(&request.sequence)
                    .map(|(outcome, _)| outcome)
            });
        if outcome.is_some_and(|entry| entry.request != *request) {
            return Err(Error::HandoverState);
        }
        Ok(outcome.cloned())
    }
    fn phase(&self, phase: Phase) -> Result<()> {
        let mut state = lock(&self.state)?;
        state.phase = phase;
        if let Some(current) = &mut state.current {
            current.phase = phase;
            current.fenced |= phase == Phase::Fenced;
        }
        Ok(())
    }
    fn checkpoint(&self, deadline: Instant) -> Result<()> {
        if self.closing.load(Ordering::SeqCst) {
            return Err(Error::Draining);
        }
        if Instant::now() >= deadline {
            return Err(Error::HandoverBusy);
        }
        Ok(())
    }
    /// Latch closure and clean up exact retained handles, outside all state locks.
    /// Retry after an adapter cleanup error; no handle or obligation is discarded.
    /// Both workers are attempted even if one fails; only failed handles remain.
    /// # Errors
    /// Returns `HandoverBusy` while a replacement callback owns the operation;
    /// wait for that operation to return, then retry until cleanup is confirmed.
    /// adapter errors retain handles and block further dispatch/replacement.
    pub fn shutdown(&self) -> Result<()> {
        self.close();
        let _operation = self.operation.try_lock().map_err(|_| Error::HandoverBusy)?;
        self.cleanup()
    }
    fn cleanup(&self) -> Result<()> {
        let deadline = Instant::now() + self.options.cleanup;
        let (candidate, active) = {
            let state = lock(&self.state)?;
            (state.candidate.clone(), state.active.clone())
        };
        let mut failure = None;
        for (candidate, slot) in [(true, candidate), (false, active)] {
            if let Some(slot) = slot {
                if let Err(error) = transition::callback(|| slot.worker.stop(deadline)) {
                    failure.get_or_insert(error);
                    continue;
                }
                let mut state = lock(&self.state)?;
                if candidate {
                    state.candidate = None;
                } else {
                    state.active = None;
                }
            }
        }
        if let Some(error) = failure {
            self.phase(Phase::Reconcile)?;
            return Err(error);
        }
        self.phase(Phase::Stopped)
    }
}

const OUTCOME_LIMIT: usize = 64 * 1024;
