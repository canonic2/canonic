//! The manager: one per user and data root, serving every client and device.
//! It accepts same-user client connections, admits requests by role, keeps
//! the device pool, runs device actors under its supervisor and exits only
//! when no actor has an obligation and no client connection is open, after an
//! atomic re-check that closes admission first.
use super::{CONTROL, Caller, PROTOCOL, storage, supervisor::Supervisor, wire};
use crate::error::{Error, Result};
use canonic_process_kit::{
    connection::Connection,
    core::{Admission as Gate, Hold},
};
pub(crate) use lifetime::retrying;
use registry::{Kind, Registry};
pub(crate) use requests::not_connected;
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

mod builds;
mod diagnostics;
mod lifetime;
mod recovery;
pub(crate) mod registry;
mod requests;
#[cfg(test)]
mod tests;

/// How long an empty manager waits after its last use before exiting,
/// so consecutive commands reuse it. Inspection does not extend it.
const IDLE_GRACE: Duration = Duration::from_secs(3);

/// Independent reads one target admits at a time.
const TARGET_READS: usize = 4;

/// Admission limits: per target, plus owner-wide bounds that protect the host
/// and admit the per-target allowance on eight targets at once.
pub(crate) struct Admission {
    calls: Arc<tokio::sync::Semaphore>,
    reads: Arc<tokio::sync::Semaphore>,
    inspect: Arc<tokio::sync::Semaphore>,
    target_reads: Mutex<HashMap<String, Arc<tokio::sync::Semaphore>>>,
}
fn overloaded(what: &str) -> Error {
    Error::new(
        "OVERLOADED",
        format!("The manager's {what} capacity is full"),
    )
    .details(json!({"admitted":false,"dispatched":false}))
}
impl Admission {
    fn new() -> Self {
        Self {
            calls: Arc::new(tokio::sync::Semaphore::new(64)),
            reads: Arc::new(tokio::sync::Semaphore::new(TARGET_READS * 8)),
            inspect: Arc::new(tokio::sync::Semaphore::new(8)),
            target_reads: Mutex::default(),
        }
    }
    pub(crate) fn admit(&self) -> Result<tokio::sync::OwnedSemaphorePermit> {
        self.calls
            .clone()
            .try_acquire_owned()
            .map_err(|_| overloaded("request"))
    }
    /// One independent read of `target`, within its own allowance and the
    /// owner-wide bound.
    pub(crate) fn read(&self, target: Option<&str>) -> Result<ReadSlot> {
        let owner = self
            .reads
            .clone()
            .try_acquire_owned()
            .map_err(|_| overloaded("read"))?;
        let target = match target {
            Some(target) => Some(
                self.target_reads
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .entry(target.to_owned())
                    .or_insert_with(|| Arc::new(tokio::sync::Semaphore::new(TARGET_READS)))
                    .clone()
                    .try_acquire_owned()
                    .map_err(|_| {
                        Error::new("OVERLOADED", "This device's read capacity is full").details(
                            json!({"admitted":false,"dispatched":false,"limit":TARGET_READS}),
                        )
                    })?,
            ),
            None => None,
        };
        Ok(ReadSlot {
            _owner: owner,
            _target: target,
        })
    }
    /// Forget per-target read allowances no read holds: a permit keeps its
    /// semaphore alive, so pruning never drops one in use.
    fn prune(&self) {
        self.target_reads
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .retain(|_, reads| Arc::strong_count(reads) > 1);
    }
    /// The bounded in-flight slot of an inspect or background request.
    pub(crate) fn inspect(&self) -> Result<tokio::sync::OwnedSemaphorePermit> {
        self.inspect
            .clone()
            .try_acquire_owned()
            .map_err(|_| overloaded("inspection"))
    }
}

/// An admitted independent read's owner-wide and per-target slots.
pub(crate) struct ReadSlot {
    _owner: tokio::sync::OwnedSemaphorePermit,
    _target: Option<tokio::sync::OwnedSemaphorePermit>,
}

/// One pooled target's FIFO queue for device-changing work from every client.
#[derive(Default)]
pub(crate) struct Queue {
    lock: Arc<tokio::sync::Mutex<()>>,
    waiting: AtomicUsize,
}
/// A request's turn in its target's queue. It keeps the queue itself, so
/// pruning cannot replace the queue of a target whose request is running.
pub(crate) struct Turn {
    _guard: tokio::sync::OwnedMutexGuard<()>,
    _queue: Arc<Queue>,
}
impl Queue {
    /// Wait for this target's turn, applying a caller deadline when set. The mutex is fair,
    /// so turns follow arrival order.
    pub(crate) async fn turn(
        self: Arc<Self>,
        deadline: Option<tokio::time::Instant>,
    ) -> Option<Turn> {
        self.waiting.fetch_add(1, Ordering::SeqCst);
        let waiting = Waiting(&self.waiting);
        let acquire = self.lock.clone().lock_owned();
        let guard = match deadline {
            Some(deadline) => tokio::time::timeout_at(deadline, acquire).await.ok()?,
            None => acquire.await,
        };
        drop(waiting);
        Some(Turn {
            _guard: guard,
            _queue: self,
        })
    }
    #[cfg(all(test, unix))]
    pub(crate) fn waiting(&self) -> usize {
        self.waiting.load(Ordering::SeqCst)
    }
}

struct Waiting<'a>(&'a AtomicUsize);

impl Drop for Waiting<'_> {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

/// Results of admitted requests by stable request ID: a request resent after
/// an ambiguous transport failure joins or returns the original.
#[derive(Default)]
struct Requests {
    results: HashMap<String, tokio::sync::watch::Receiver<Option<Value>>>,
    order: std::collections::VecDeque<String>,
}

pub(crate) struct Service {
    pub(crate) root: PathBuf,
    pub(crate) incarnation: String,
    protocol: u32,
    started: u64,
    registry: Mutex<Registry>,
    pub(crate) supervisor: Arc<Supervisor>,
    pub(crate) admission: Admission,
    pub(crate) browsers: crate::browser::runtime::Owners,
    pub(crate) builds: crate::builds::Owners,
    pub(crate) diagnostics: crate::diagnostics::Owners,
    queues: Mutex<HashMap<String, Arc<Queue>>>,
    requests: Mutex<Requests>,
    /// Release attempts in progress by target: concurrent callers share one.
    releases: Mutex<HashMap<String, tokio::sync::watch::Receiver<Option<Result<Value>>>>>,
    /// The library gate and busy hold once the manager is hosted.
    busy: std::sync::OnceLock<Busy>,
    changed: tokio::sync::Notify,
    /// Resources a lost manager left that settlement could not release.
    lost: Mutex<Vec<Value>>,
    /// Serializes pool record writes.
    pool_records: Mutex<()>,
    /// The budget of one recovery attempt before it answers pending.
    recovery_budget: Duration,
    idle_grace: Duration,
    /// How long a connected target stays without use before its release.
    idle_release: Duration,
}

impl Service {
    pub(crate) fn new(root: PathBuf, launcher: super::supervisor::Launcher) -> Arc<Self> {
        let incarnation = uuid::Uuid::new_v4().to_string();
        Arc::new(Self {
            root,
            incarnation: incarnation.clone(),
            protocol: PROTOCOL,
            started: super::now(),
            registry: Mutex::default(),
            supervisor: Arc::new(Supervisor::new(launcher)),
            admission: Admission::new(),
            browsers: crate::browser::runtime::Owners::default(),
            builds: crate::builds::Owners::default(),
            diagnostics: crate::diagnostics::Owners::new(incarnation),
            queues: Mutex::default(),
            requests: Mutex::default(),
            releases: Mutex::default(),
            busy: std::sync::OnceLock::new(),
            changed: tokio::sync::Notify::new(),
            lost: Mutex::default(),
            pool_records: Mutex::default(),
            recovery_budget: Duration::from_secs(45),
            idle_grace: IDLE_GRACE,
            idle_release: registry::IDLE_RELEASE,
        })
    }
    /// A fixture service with configurable protocol and time budgets. Unix can
    /// also run a retained device actor through its native test socket.
    #[cfg(test)]
    pub(crate) fn fixture(root: PathBuf, protocol: u32, recovery_budget: Duration) -> Arc<Self> {
        #[cfg(unix)]
        let launcher = super::supervisor::Launcher::Fixture;
        #[cfg(not(unix))]
        let launcher = super::supervisor::Launcher::Product;
        let mut service = Self::new(root, launcher);
        let inner = Arc::get_mut(&mut service).unwrap();
        inner.protocol = protocol;
        inner.recovery_budget = recovery_budget;
        inner.idle_grace = Duration::from_millis(200);
        service
    }
    /// A fixture service whose connected targets are released after `idle`.
    #[cfg(all(test, unix))]
    pub(crate) fn fixture_idle(root: PathBuf, idle: Duration) -> Arc<Self> {
        let mut service = Self::fixture(root, PROTOCOL, Duration::from_secs(45));
        Arc::get_mut(&mut service).unwrap().idle_release = idle;
        service
    }
    /// One admitted request, without the socket or the library's admission.
    #[cfg(test)]
    pub(crate) async fn request(self: &Arc<Self>, request: Value) -> Result<Value> {
        let service = self.clone();
        detached(move || async move { service.dispatch(request).await })
            .await
            .unwrap_or_else(|_| Err(Error::new("INTERNAL", "The request stopped unexpectedly")))
    }
    fn registry(&self) -> std::sync::MutexGuard<'_, Registry> {
        self.registry
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
    fn notify(&self) {
        self.changed.notify_waiters();
    }
    /// Wait until `condition` holds over the registry, rechecking on change.
    async fn wait_registry(&self, condition: impl Fn(&Registry) -> bool) {
        loop {
            let changed = self.changed.notified();
            tokio::pin!(changed);
            changed.as_mut().enable();
            if condition(&self.registry()) {
                return;
            }
            // Actors and records change without a registry event; recheck.
            let _ = tokio::time::timeout(Duration::from_millis(250), changed).await;
        }
    }

    // Pool operations used by callers inside this process.

    /// The FIFO queue every client shares for one target; different targets
    /// proceed independently.
    pub(crate) fn queue(&self, target: Option<&str>) -> Arc<Queue> {
        self.queues
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .entry(target.unwrap_or_default().to_owned())
            .or_default()
            .clone()
    }
    /// The pooled target `target` names, resolving an alias.
    pub(crate) fn held_target(&self, target: &str) -> Option<String> {
        self.registry().pool()?.primary(target)
    }
    pub(super) fn connected_target(&self, target: &str) -> bool {
        self.registry().pool().is_some_and(|pool| {
            pool.primary(target)
                .and_then(|target| pool.targets.get(&target))
                .is_some_and(|held| held.connected)
        })
    }
    /// `target` left the pool after a verified release.
    pub(crate) fn release_target(&self, target: &str) {
        self.diagnostics.stop_all(Some(target), "target_released");
        self.registry().release_target(target);
        self.prune();
        self.notify();
    }
    /// How many target queues and read allowances the owner keeps.
    #[cfg(test)]
    pub(crate) fn tracked(&self) -> (usize, usize) {
        let queues = self
            .queues
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .len();
        let reads = self
            .admission
            .target_reads
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .len();
        (queues, reads)
    }
    /// Forget queues and read allowances no request holds, so these maps
    /// stay bounded by the work in progress while the manager keeps
    /// running. A waiting or running request holds its queue, so pruning
    /// never splits one target's FIFO order.
    pub(crate) fn prune(&self) {
        self.queues
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .retain(|_, queue| Arc::strong_count(queue) > 1);
        self.admission.prune();
    }
    /// Pooled targets, devices and browsers alike, connected or claimed.
    #[cfg(test)]
    pub(crate) fn targets(&self) -> Vec<String> {
        self.registry()
            .pool()
            .map(|pool| pool.targets.keys().cloned().collect())
            .unwrap_or_default()
    }
    /// Connected device targets.
    pub(crate) fn devices(&self) -> Vec<String> {
        self.registry()
            .pool()
            .map(registry::Pool::devices)
            .unwrap_or_default()
    }
    /// Connect `target` in the pool for every client.
    pub(crate) fn claim(&self, target: &str) -> Result<()> {
        let result = self.registry().claim(target);
        self.notify();
        result
    }
    /// Run `start` in one registry step with a check that the manager is not
    /// exiting, so an exit either finds what `start` began or `start` never runs.
    pub(crate) fn start_if_open(&self, start: impl FnOnce() -> Result<()>) -> Result<()> {
        let registry = self.registry();
        if registry.closing || registry.pool().is_none() {
            return Err(Error::new(
                "OWNER_EXITING",
                "The manager is exiting; the device was not started",
            )
            .details(json!({"admitted":true,"dispatched":false})));
        }
        start()
    }
    pub(crate) fn transition(&self, from: &str, to: &str) -> Result<()> {
        self.registry().transition(from, to)
    }
    /// The pool's generation and records directory, for browser connections.
    pub(crate) fn browser_identity(&self) -> Result<(u64, PathBuf)> {
        self.registry()
            .pool()
            .map(|pool| (pool.generation, pool.directory.clone()))
            .ok_or_else(|| Error::new("OWNER_EXITING", "The manager has no device pool"))
    }
    pub(crate) fn pool_summary(&self) -> Option<Value> {
        self.registry().pool().map(registry::Pool::summary)
    }
    /// A viewer took or gave back `target`'s input.
    pub(crate) fn viewer(&self, target: &str, holding: bool) {
        self.registry().viewer(target, holding);
        self.notify();
    }
    /// The pending release of `target`, when an earlier one failed.
    pub(crate) fn pending_release(&self, target: &str) -> Option<registry::Release> {
        self.registry().pending_release(target)
    }
    /// A connect released `target`'s previous actor: no release is pending.
    pub(crate) fn release_settled(&self, target: &str) {
        self.registry().release_settled(target);
        self.notify();
    }
    /// Publish offline status and preserve the result of the browser request.
    /// Report a record failure alongside a successful result so the caller
    /// still receives the affected target ID.
    pub(super) fn with_browser_record(
        &self,
        id: &str,
        directory: &Path,
        result: Result<Value>,
    ) -> Result<Value> {
        let record_error = self.record_browsers(id, directory).err();
        result.map(|mut value| {
            if let (Some(fields), Some(error)) = (value.as_object_mut(), record_error) {
                fields.insert("statusRecordError".into(), json!(error));
            }
            value
        })
    }
    /// Publish the current browser list for offline status. Serialize the
    /// snapshot with its write so an older request cannot restore a closed row.
    fn record_browsers(&self, id: &str, directory: &Path) -> Result<()> {
        let _serialized = self
            .pool_records
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let browsers = self.browsers.snapshots(id);
        let path = directory.join("browser status.json");
        if browsers.is_empty() && !path.exists() {
            return Ok(());
        }
        storage::write(&path, &json!({"browsers":browsers}))
    }
    fn persist(&self) {
        let record = {
            let registry = self.registry();
            registry.pool().map(|pool| {
                let mut summary = pool.summary();
                summary["ownerIncarnation"] = json!(self.incarnation);
                summary["updated"] = json!(super::now());
                (pool.directory.clone(), summary)
            })
        };
        if let Some((directory, summary)) = record {
            let _serialized = self
                .pool_records
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let _ = storage::write(&directory.join("pool.json"), &summary);
        }
    }
    /// A caller for an admitted request in the pool.
    fn caller(self: &Arc<Self>, kind: Kind) -> Option<Caller> {
        let id = self.registry().pool()?.id.clone();
        let mut caller = Caller::new(Some(self.clone()), self.root.clone(), kind);
        caller.pool = Some(id);
        Some(caller)
    }
    /// The pool, opened when the manager has none, with `then` applied in the
    /// same registry step so a new pool gains its first request before it
    /// could be found empty.
    fn open_then<T>(&self, then: impl FnOnce(&mut Registry) -> Result<T>) -> Result<T> {
        let (value, opened) = {
            let mut registry = self.registry();
            let opened = registry.pool().is_none();
            registry.open(|| {
                let directory =
                    super::selection::pool_directory(&self.root, super::identity::POOL)?;
                let generation = storage::read::<Value>(&directory.join("pool.json"))?
                    .and_then(|record| record["generation"].as_u64())
                    .unwrap_or(0)
                    + 1;
                Ok(registry::Opening {
                    directory,
                    generation,
                })
            })?;
            (then(&mut registry)?, opened)
        };
        if opened {
            self.persist();
        }
        Ok(value)
    }

    /// Replace the report of what a lost manager left unsettled.
    pub(crate) fn set_lost(&self, blockers: Vec<Value>) {
        *self
            .lost
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = blockers;
    }
    pub(crate) fn identity(&self) -> Value {
        let registry = self.registry();
        json!({
            "incarnation":self.incarnation,"protocol":self.protocol,"control":CONTROL,
            "capabilities":wire::CAPABILITIES,"build":super::BUILD_ID,"pid":std::process::id(),
            "started":self.started,"draining":registry.draining,
            "targets":registry.pool().map_or(0, |pool| pool.targets.len()),"actors":self.supervisor.alive(),
        })
    }
}

/// Run the future `work` builds on the blocking pool, driven by the owner
/// process's runtime: requests, session ends and recovery attempts do
/// durable record I/O and polling waits that must not occupy an async worker.
/// The future is built on that thread, so it need not be `Send`.
pub(crate) fn detached<F>(
    work: impl FnOnce() -> F + Send + 'static,
) -> tokio::task::JoinHandle<F::Output>
where
    F: std::future::Future,
    F::Output: Send + 'static,
{
    let runtime = tokio::runtime::Handle::current();
    tokio::task::spawn_blocking(move || runtime.block_on(work()))
}

/// The library admission gate of a hosted manager and the one hold that keeps
/// it running while Testrun has an obligation: a pooled target, a live actor,
/// an active build or a release in progress.
struct Busy {
    gate: Gate,
    hold: Mutex<Option<Hold>>,
}

impl Service {
    pub(crate) fn protocol(&self) -> u32 {
        self.protocol
    }
    pub(crate) fn idle_grace(&self) -> Duration {
        self.idle_grace
    }
    /// Serve under the library gate `gate`: it decides exit, retirement and drain.
    pub(crate) fn host(&self, gate: Gate) {
        let _ = self.busy.set(Busy {
            gate,
            hold: Mutex::new(None),
        });
    }
    /// Whether Testrun holds nothing that must outlive this process.
    fn obligations_settled(&self, registry: &Registry) -> bool {
        !self.builds.active()
            && !self.diagnostics.active()
            && registry.is_empty()
            && self.supervisor.alive() == 0
            && self
                .releases
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .is_empty()
    }
    /// Match the busy hold to Testrun's obligations. An admitted request
    /// passes its permit, which may take the hold even while draining; the
    /// lifetime loop passes none and mostly lets the hold go.
    /// Match the busy hold to Testrun's obligations. Work that outlived its
    /// request passes the hold it carried: splitting it keeps the manager
    /// even while draining, when the gate takes no new hold.
    pub(crate) fn keep(&self, carried: Option<&Hold>) {
        let Some(busy) = self.busy.get() else {
            return;
        };
        let mut hold = busy
            .hold
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if self.obligations_settled(&self.registry()) {
            *hold = None;
        } else if hold.is_none() {
            *hold = match carried {
                Some(carried) => Some(carried.split()),
                None => busy.gate.hold().ok(),
            };
        }
    }
    /// A hold for work that starts outside any request, such as recovery at
    /// startup; `None` when the manager is not hosted or admission closed.
    pub(crate) fn hold(&self) -> Option<Hold> {
        self.busy.get().and_then(|busy| busy.gate.hold().ok())
    }
    #[cfg(all(test, unix))]
    pub(crate) fn busy_held(&self) -> bool {
        self.busy.get().is_some_and(|busy| {
            busy.hold
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .is_some()
        })
    }
    /// Let the library exit once admitted work settles, after signal cleanup.
    pub(crate) fn release_hold(&self) {
        if let Some(busy) = self.busy.get() {
            *busy
                .hold
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
        }
    }
    /// The library closed admission and exited: nothing may start now.
    pub(crate) fn closed(&self) {
        let mut registry = self.registry();
        registry.closing = true;
        registry.close();
    }
    /// Best-effort exit cleanup on a signal. A failed browser close never
    /// skips the device actors; it is reported after all cleanup ran.
    pub(crate) async fn terminate(&self) -> Result<()> {
        self.registry().closing = true;
        self.diagnostics.stop_all(None, "manager_stopping");
        let pool = self.registry().pool().map(|pool| pool.id.clone());
        let mut failed = None;
        if tokio::time::timeout(
            Duration::from_secs(30),
            self.wait_registry(|_| !self.diagnostics.active()),
        )
        .await
        .is_err()
        {
            failed = Some(Error::new(
                "CLEANUP_PENDING",
                "Diagnostic finalization is pending; retained status will preserve partial evidence after restart",
            ));
        }
        if let Some(id) = pool
            && let Err(error) = self.browsers.close(&id).await
        {
            failed.get_or_insert(error);
        }
        self.supervisor.terminate();
        // Started blocking work may delay cleanup; it is bounded.
        let _ = tokio::time::timeout(Duration::from_secs(90), self.supervisor.finished_all()).await;
        failed.map_or(Ok(()), Err)
    }
    /// What keeps the manager from retiring, for the library's retirement
    /// and status replies.
    pub(crate) fn holders(&self) -> Value {
        json!({"pool":self.pool_summary(),"actors":self.supervisor.inventory(),"diagnosticCollectors":self.diagnostics.active_count(),"diagnosticAnalyses":self.diagnostics.active_analysis_count()})
    }
    /// Run one admitted request off the async workers, so its record I/O and
    /// waits never hold one. Dropping the returned future cancels the
    /// request just as dropping it in place would.
    ///
    /// `hold` keeps a hosted manager running until this request's work ends,
    /// even when its client abandons it, and hands over to the busy hold if
    /// the work left a device or actor behind.
    pub(crate) async fn offload(
        self: &Arc<Self>,
        request: Value,
        hold: Option<Hold>,
    ) -> Result<Value> {
        let (cancel, cancelled) = tokio::sync::oneshot::channel::<()>();
        let service = self.clone();
        let task = detached(move || async move {
            let result = tokio::select! {
                result = service.dispatch(request) => Some(result),
                _ = cancelled => None,
            };
            service.keep(hold.as_ref());
            result
        });
        let result = task.await;
        drop(cancel);
        match result {
            Ok(Some(result)) => result,
            Ok(None) => Err(Error::new("CANCELLED", "The client abandoned the request")),
            Err(_) => Err(Error::new("INTERNAL", "The request stopped unexpectedly")),
        }
    }
    async fn dispatch(self: &Arc<Self>, request: Value) -> Result<Value> {
        let Some(request_id) = request["requestId"].as_str() else {
            return self.perform(request).await;
        };
        // The same ID with different content is not a resend: it reaches the
        // workflow, whose receipts refuse a conflicting command.
        let id = format!(
            "{request_id}:{}",
            super::hash(json!([request["kind"], request["name"], request["args"]]).to_string())
        );
        let joined = {
            let mut requests = self
                .requests
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if let Some(receiver) = requests.results.get(&id) {
                Err(receiver.clone())
            } else {
                let (sender, receiver) = tokio::sync::watch::channel(None);
                requests.results.insert(id.clone(), receiver);
                requests.order.push_back(id.clone());
                while requests.order.len() > 512 {
                    if let Some(old) = requests.order.pop_front() {
                        requests.results.remove(&old);
                    }
                }
                Ok(sender)
            }
        };
        match joined {
            Ok(sender) => {
                let result = self.perform(request).await;
                sender.send_replace(Some(wire::frame(&result)));
                // A refusal admitted nothing: the same request may be sent
                // again, for example after the client attaches.
                if result.as_ref().is_err_and(|error| {
                    error
                        .details
                        .as_ref()
                        .is_some_and(|details| details["admitted"] == false)
                }) {
                    self.requests
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .results
                        .remove(&id);
                }
                result
            }
            Err(mut receiver) => {
                // The same request is running or finished: never repeat it.
                let frame = receiver
                    .wait_for(Option::is_some)
                    .await
                    .map(|frame| frame.clone().unwrap_or(Value::Null))
                    .map_err(|_| {
                        Error::new(
                            "OUTCOME_UNKNOWN",
                            "The original request was abandoned; inspect status",
                        )
                    })?;
                wire::result(&frame)
            }
        }
    }
    /// Product requests are calls; controls and task end arrive on the
    /// library's control lane instead.
    async fn perform(self: &Arc<Self>, request: Value) -> Result<Value> {
        match request["kind"].as_str() {
            Some("call") => self.call(request).await,
            _ => Err(Error::invalid("Unsupported owner request")),
        }
    }
    /// An MCP server's handed-off connection: its library guard keeps the
    /// manager running until the socket's EOF, and it holds nothing else.
    /// Its EOF releases no device. A draining manager lets its MCP servers
    /// go, so they cannot hold the drain open; their next request finds its
    /// successor.
    pub(crate) async fn attach(self: Arc<Self>, connection: Connection) {
        let (stream, guard) = connection.into_parts();
        tokio::select! {
            () = super::host::peer_closed(Some(stream)) => {}
            () = self.wait_registry(|registry| registry.draining) => {}
        }
        drop(guard);
        self.notify();
    }
}
