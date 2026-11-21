//! Opt-in implementation processes behind the existing MCP drivers.
//!
//! The frontend retains MCP state. Consumers supply authenticated native process
//! adapters implementing [`Implementation`]; Process Kit owns exclusive cutover.
//! Installing an executable alone does not trigger replacement.

use crate::{Catalog, Change, SessionOptions, Tool, json, stdio};
use canonic_process_kit::{
    self as process, endpoint::Lease, protocol::Identity, worker as lifecycle,
};
use serde_json::Value;
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    thread::Thread,
    time::{Duration, Instant},
};

pub mod wire;

pub use lifecycle::{
    Commit, Options as SupervisorOptions, Outcome, Phase, Request, Selection, Status, Transition,
};

/// Why a process frontend could not reserve, replace or shut down.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Invalid limits or a different resident frontend contract.
    #[error("invalid or incompatible MCP worker contract")]
    Contract,
    /// One frontend is attached already, or replacement owns publication.
    #[error("MCP worker frontend is busy")]
    Busy,
    /// The underlying controller retains the exact process handles on failure.
    #[error(transparent)]
    Process(#[from] process::Error),
}

/// Execution certainty after a private-channel failure. Neither variant proves
/// side effects were rolled back, and neither permits replay.
#[derive(Debug, thiserror::Error)]
pub enum Failure {
    /// The adapter proved execution has ended (for example by exact reaping).
    #[error("worker exchange failed after execution settled")]
    Settled(crate::RpcError),
    /// Execution may still be live. Keep a process obligation until shutdown
    /// proves cleanup; ordinary replacement must defer.
    #[error("worker exchange failed with execution still uncertain")]
    Uncertain(crate::RpcError),
}
impl From<crate::RpcError> for Failure {
    fn from(error: crate::RpcError) -> Self {
        Self::Uncertain(error)
    }
}

/// Immutable frontend declarations and resident policy identity.
/// Construct from the actual driver options; candidates must supply the same
/// complete value. `resident` includes identity, callback policy, embedded docs
/// and consumer state-format declarations that cannot be inferred from Rust code.
#[derive(Clone, PartialEq)]
pub struct Contract {
    tools: Vec<Tool>,
    session: SessionOptions,
    lanes: Vec<stdio::Lane>,
    timeout: Duration,
    disconnect_grace: Duration,
    output: usize,
    elicitation: u8,
    resident: Value,
}

impl Contract {
    /// Capture complete prepared declarations, without catalog pagination.
    /// # Errors
    /// Refuses an invalid resident declaration or its explicit encoded budget.
    pub fn new(
        catalog: &Catalog,
        options: &stdio::Options,
        resident: Value,
        bytes: usize,
    ) -> Result<Self, Error> {
        if bytes == 0
            || !resident.is_object()
            || json::measure(&resident, bytes, &|| Ok(())).is_err()
        {
            return Err(Error::Contract);
        }
        Ok(Self {
            tools: catalog.tools().to_vec(),
            session: options.session.clone(),
            lanes: options.lanes.clone(),
            timeout: options.timeout,
            disconnect_grace: options.disconnect_grace,
            output: options.max_output_bytes,
            elicitation: options.elicitation.max_rounds,
            resident,
        })
    }
    pub(crate) fn matches(&self, catalog: &Catalog, options: &stdio::Options) -> bool {
        self.tools == catalog.tools()
            && self.session == options.session
            && self.lanes == options.lanes
            && self.timeout == options.timeout
            && self.disconnect_grace == options.disconnect_grace
            && self.output == options.max_output_bytes
            && self.elicitation == options.elicitation.max_rounds
    }
    fn changed(&mut self, change: &Change) {
        match change {
            Change::Tools(catalog) => self.tools = catalog.tools().to_vec(),
            Change::Prompts(catalog) => self.session.prompts = Some(Arc::clone(catalog)),
            Change::Resources(catalog) => self.session.resources = Some(Arc::clone(catalog)),
            Change::ResourceUpdated(_) => (),
        }
    }
}

/// A consumer-owned authenticated process and its MCP request adapter.
///
/// Implementations send typed work through private bounded channels, forward
/// cancellation/progress using `context`. Successful outcomes and `Settled`
/// failures prove execution ended; unknown execution returns `Uncertain` and
/// keeps an obligation until exact cleanup. They must never replay a failed
/// exchange. Continuation closures are local proxies to the original
/// process; resource ownership must be pinned with [`stdio::Context::worker_hold`].
/// Native owner-loss, executable authorization and exact reaping follow the
/// Process Kit `Worker` contract. This trait does not make a process a sandbox.
pub trait Implementation: lifecycle::Worker {
    /// Complete immutable declaration authenticated with this actual process.
    fn contract(&self) -> &Contract;
    /// Execute one assignment. Domain errors are typed outcomes; an exchange
    /// error makes this generation unavailable without implying rollback.
    /// # Errors
    /// Report channel/protocol/process failures without replaying effects.
    fn call(
        &self,
        request: stdio::Request,
        context: &stdio::Context,
    ) -> Result<stdio::Outcome, Failure>;
}

struct Process {
    implementation: Arc<dyn Implementation>,
    contract: Arc<Mutex<Contract>>,
    reservations: Arc<AtomicUsize>,
    failed: AtomicBool,
}

impl lifecycle::Worker for Process {
    fn identity(&self) -> &Identity {
        self.implementation.identity()
    }
    fn exited(&self) -> process::Result<bool> {
        self.implementation.exited()
    }
    fn prepare(&self, transition: &Transition<'_>) -> process::Result<()> {
        if self.implementation.contract()
            != &*self.contract.lock().map_err(|_| process::Error::Worker)?
        {
            return Err(process::Error::Incompatible);
        }
        self.implementation.prepare(transition)
    }
    fn fence(&self, transition: &Transition<'_>) -> process::Result<()> {
        self.implementation.fence(transition)
    }
    fn retire(&self, transition: &Transition<'_>) -> process::Result<()> {
        self.implementation.retire(transition)
    }
    fn activate(&self, transition: &Transition<'_>) -> process::Result<()> {
        self.implementation.activate(transition)
    }
    fn stop(&self, deadline: Instant) -> process::Result<()> {
        // EOF must not kill already admitted read-only work. Cleanup retries
        // after the scheduler releases queued/running/continuation owners.
        if self.reservations.load(Ordering::Acquire) != 0 {
            return Err(process::Error::HandoverBusy);
        }
        self.implementation.stop(deadline)
    }
}

/// A stable frontend shared with one stdio connection or one HTTP server.
/// Replacement runs on the caller's control thread, never the dispatcher.
pub struct Frontend {
    supervisor: lifecycle::Supervisor<Process>,
    contract: Arc<Mutex<Contract>>,
    control: Mutex<()>,
    operation: Mutex<()>,
    attached: AtomicBool,
    dispatcher: Mutex<Option<Thread>>,
    bytes: AtomicUsize,
    holds: Arc<AtomicUsize>,
    max_bytes: usize,
    max_holds: usize,
    capacity: usize,
    uncertain: Mutex<Option<lifecycle::Hold<Process>>>,
}

impl Frontend {
    /// Install an already active implementation while retaining kernel exclusion.
    /// `max_bytes` bounds all retained process-bound input, not just paused work.
    /// # Errors
    /// Refuses incompatible declarations, invalid bounds or controller identities.
    pub fn new(
        lease: Lease,
        identity: Identity,
        active: Arc<dyn Implementation>,
        options: SupervisorOptions,
        max_bytes: usize,
        max_holds: usize,
    ) -> Result<Arc<Self>, Error> {
        if max_bytes == 0 || max_holds == 0 {
            return Err(Error::Contract);
        }
        let declared =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| active.contract().clone()))
                .map_err(|_| Error::Contract)?;
        let contract = Arc::new(Mutex::new(declared));
        let process = Process {
            failed: AtomicBool::new(false),
            implementation: active,
            contract: Arc::clone(&contract),
            reservations: Arc::new(AtomicUsize::new(0)),
        };
        Ok(Arc::new(Self {
            supervisor: lifecycle::Supervisor::new(lease, identity, process, options)?,
            contract,
            control: Mutex::new(()),
            operation: Mutex::new(()),
            attached: AtomicBool::new(false),
            dispatcher: Mutex::new(None),
            bytes: AtomicUsize::new(0),
            holds: Arc::new(AtomicUsize::new(0)),
            max_bytes,
            max_holds,
            capacity: options.capacity,
            uncertain: Mutex::new(None),
        }))
    }
    /// Prepare, settle and exclusively activate an authorized candidate.
    /// No host work is cancelled to make an update fit the preparation budget.
    /// # Errors
    /// Refuses competing attempts; other failures retain controller certainty.
    pub fn replace(
        &self,
        request: &Request,
        launch: impl FnOnce(Instant) -> process::Result<Arc<dyn Implementation>>,
        commit: impl FnOnce(&Transition<'_>) -> process::Result<()>,
    ) -> Result<Outcome, Error> {
        let control = self.control.try_lock().map_err(|_| Error::Busy)?;
        // Polling may briefly own publication, but only another control attempt
        // makes replacement busy. Never make the dispatcher wait for cutover.
        let operation = self.operation.lock().map_err(|_| Error::Busy)?;
        let result = self.supervisor.replace(
            request,
            |deadline| {
                Ok(Process {
                    failed: AtomicBool::new(false),
                    implementation: launch(deadline)?,
                    contract: Arc::clone(&self.contract),
                    reservations: Arc::new(AtomicUsize::new(0)),
                })
            },
            commit,
        );
        drop(operation);
        drop(control);
        self.wake();
        Ok(result?)
    }
    /// Inspect bounded status without process I/O.
    /// # Errors
    /// Returns poisoned controller state.
    pub fn status(&self) -> Result<Status, Error> {
        Ok(self.supervisor.status()?)
    }
    /// Inspect an exact retained attempt without reissuing effects.
    /// # Errors
    /// Refuses changed, foreign or evicted request identities.
    pub fn outcome(&self, request: &Request) -> Result<Outcome, Error> {
        Ok(self.supervisor.outcome(request)?)
    }
    /// Latch host closure immediately. Existing reservations keep source work alive.
    pub fn close(&self) {
        self.supervisor.close();
        self.wake();
    }
    /// Stop exact owned processes; retry after busy or uncertain cleanup.
    /// # Errors
    /// Keeps handles on a competing attempt or native cleanup failure.
    pub fn shutdown(&self) -> Result<(), Error> {
        self.close();
        let _control = self.control.try_lock().map_err(|_| Error::Busy)?;
        let _operation = self.operation.lock().map_err(|_| Error::Busy)?;
        self.supervisor.shutdown()?;
        self.uncertain.lock().map_err(|_| Error::Busy)?.take();
        Ok(())
    }
    pub(crate) fn finish(&self) -> Result<(), Error> {
        self.close();
        let _control = self.control.lock().map_err(|_| Error::Busy)?;
        let _operation = self.operation.lock().map_err(|_| Error::Busy)?;
        self.supervisor.shutdown()?;
        self.uncertain.lock().map_err(|_| Error::Busy)?.take();
        Ok(())
    }
    fn wake(&self) {
        if let Ok(dispatcher) = self.dispatcher.lock()
            && let Some(dispatcher) = &*dispatcher
        {
            dispatcher.unpark();
        }
    }
    pub(crate) fn attach(
        self: &Arc<Self>,
        catalog: &Catalog,
        options: &stdio::Options,
    ) -> Result<Attachment, Error> {
        let capacity = options
            .lanes
            .iter()
            .try_fold(0usize, |sum, lane| sum.checked_add(lane.capacity));
        if self.supervisor.status()?.closing {
            return Err(Error::Busy);
        }
        if !self
            .contract
            .lock()
            .map_err(|_| Error::Contract)?
            .matches(catalog, options)
            || capacity.is_none_or(|capacity| capacity > self.capacity)
        {
            return Err(Error::Contract);
        }
        self.attached
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| Error::Busy)?;
        *self.dispatcher.lock().map_err(|_| Error::Busy)? = Some(std::thread::current());
        Ok(Attachment(Arc::clone(self)))
    }
    pub(crate) fn reserve(
        self: &Arc<Self>,
        request: &stdio::Request,
    ) -> Result<Arc<Reservation>, Error> {
        let bytes = wire::request_bytes(request, self.max_bytes).map_err(|error| match error {
            json::EncodeError::TooLarge { .. } => Error::Busy,
            _ => Error::Contract,
        })?;
        reserve(&self.bytes, bytes, self.max_bytes)?;
        match self.supervisor.enter() {
            Ok(dispatch) => {
                if dispatch.worker().failed.load(Ordering::Acquire) {
                    self.bytes.fetch_sub(bytes, Ordering::AcqRel);
                    return Err(process::Error::Worker.into());
                }
                dispatch
                    .worker()
                    .reservations
                    .fetch_add(1, Ordering::AcqRel);
                Ok(Arc::new(Reservation {
                    frontend: Arc::clone(self),
                    dispatch,
                    bytes,
                }))
            }
            Err(error) => {
                self.bytes.fetch_sub(bytes, Ordering::AcqRel);
                Err(error.into())
            }
        }
    }
    pub(crate) fn publish_next(
        &self,
        receiver: &stdio::ChangeReceiver,
        session: &mut crate::Session,
    ) -> Option<Result<Vec<Value>, crate::ConfigurationError>> {
        let _operation = self.operation.try_lock().ok()?;
        let status = self.supervisor.status().ok()?;
        if status.closing
            || !matches!(
                status.phase,
                Phase::Serving | Phase::Replaced | Phase::Deferred
            )
        {
            return None;
        }
        let mut contract = self.contract.lock().ok()?;
        let pending = receiver.try_recv().ok()?;
        let result = session.publish(pending.change.clone());
        if result.is_ok() {
            contract.changed(&pending.change);
        }
        Some(result)
    }
}

fn reserve(counter: &AtomicUsize, count: usize, max: usize) -> Result<(), Error> {
    counter
        .try_update(Ordering::AcqRel, Ordering::Acquire, |used| {
            used.checked_add(count).filter(|next| *next <= max)
        })
        .map(|_| ())
        .map_err(|_| Error::Busy)
}

pub(crate) struct Attachment(Arc<Frontend>);
impl Drop for Attachment {
    fn drop(&mut self) {
        self.0.close();
        self.0.attached.store(false, Ordering::Release);
    }
}

pub(crate) struct Reservation {
    frontend: Arc<Frontend>,
    dispatch: lifecycle::Dispatch<Process>,
    bytes: usize,
}
impl Reservation {
    pub(crate) fn call(&self, request: stdio::Request, context: &stdio::Context) -> stdio::Outcome {
        self.execute(|| self.dispatch.worker().implementation.call(request, context))
    }
    pub(crate) fn resume(&self, callback: impl FnOnce() -> stdio::Outcome) -> stdio::Outcome {
        self.execute(|| Ok(callback()))
    }
    fn execute(
        &self,
        callback: impl FnOnce() -> Result<stdio::Outcome, Failure>,
    ) -> stdio::Outcome {
        let process = self.dispatch.worker();
        if process.failed.load(Ordering::Acquire) {
            return stdio::Outcome::Method(Err(crate::RpcError::new(
                crate::rpc::INTERNAL_ERROR,
                "Worker generation is unavailable; request was not dispatched",
            )));
        }
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(callback));
        match result {
            Ok(Ok(outcome)) => outcome,
            Ok(Err(Failure::Settled(error))) => {
                process.failed.store(true, Ordering::Release);
                stdio::Outcome::Method(Err(error))
            }
            Ok(Err(Failure::Uncertain(error))) => {
                self.retain_uncertainty();
                process.failed.store(true, Ordering::Release);
                stdio::Outcome::Method(Err(error))
            }
            Err(_) => {
                self.retain_uncertainty();
                process.failed.store(true, Ordering::Release);
                stdio::Outcome::Method(Err(crate::RpcError::new(
                    crate::rpc::INTERNAL_ERROR,
                    "Worker exchange failed; effects may have occurred",
                )))
            }
        }
    }
    fn retain_uncertainty(&self) {
        let mut unresolved = self
            .frontend
            .uncertain
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if unresolved.is_none()
            && let Ok(hold) = self.dispatch.hold()
        {
            *unresolved = Some(hold);
        }
    }
    pub(crate) fn hold(&self) -> Result<Hold, Error> {
        reserve(&self.frontend.holds, 1, self.frontend.max_holds)?;
        match self.dispatch.hold() {
            Ok(hold) => Ok(Hold {
                budget: Arc::clone(&self.frontend.holds),
                pin: hold,
            }),
            Err(error) => {
                self.frontend.holds.fetch_sub(1, Ordering::AcqRel);
                Err(error.into())
            }
        }
    }
}
impl Drop for Reservation {
    fn drop(&mut self) {
        self.dispatch
            .worker()
            .reservations
            .fetch_sub(1, Ordering::AcqRel);
        self.frontend.bytes.fetch_sub(self.bytes, Ordering::AcqRel);
    }
}

/// Explicit worker-backed obligation that outlives an RPC. Drop only after the
/// resource/executor/producer has settled; a task handle alone is no proof.
#[must_use]
pub struct Hold {
    budget: Arc<AtomicUsize>,
    pin: lifecycle::Hold<Process>,
}
/// Generation-bound publication. Clones and queued changes retain the original
/// process, so activation cannot overtake unpublished source changes.
#[derive(Clone)]
pub struct Publisher {
    sender: stdio::ChangeSender,
    pin: Arc<Hold>,
}
impl Publisher {
    pub(crate) fn new(sender: stdio::ChangeSender, hold: Hold) -> Self {
        Self {
            sender,
            pin: Arc::new(hold),
        }
    }
    /// Enqueue without blocking or claiming host delivery.
    /// # Errors
    /// Returns the unsent change for invalid targets, saturation or closure.
    pub fn send(&self, change: Change) -> Result<(), stdio::SendChangeError> {
        self.sender.send_pinned(change, Arc::clone(&self.pin))
    }
}

impl Drop for Hold {
    fn drop(&mut self) {
        // The lifecycle hold retains the process and domain until this object drops.
        let _ = &self.pin;
        self.budget.fetch_sub(1, Ordering::AcqRel);
    }
}
