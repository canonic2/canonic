//! Real subprocess adapters around the generic replacement controller.
use super::fixtures;
use canonic_process_kit::{
    self as kit,
    core::{Admission, Explicit},
    endpoint::{Lease, Root},
    protocol::{Identity, ProductError},
    server::{Context, Handler},
    worker::{Options, Phase, Request, Selection, Supervisor, Transition, Worker},
};
use serde_json::{Value, json};
use std::{
    io::Read,
    path::Path,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

fn options() -> Options {
    Options {
        preparation: Duration::from_secs(3),
        activation: Duration::from_secs(3),
        cleanup: Duration::from_secs(3),
        capacity: 8,
        history: 2,
        history_bytes: 128 * 1024,
    }
}
fn frontend_identity() -> Identity {
    Identity::new("1.0.0".into(), 7, vec![]).unwrap()
}
fn request(frontend: &str, sequence: u64, build: &str) -> Request {
    Request {
        frontend: frontend.into(),
        sequence,
        selection: Selection {
            key: format!("acme-image-{}", build.replace('.', "-")),
            build: build.into(),
            protocol: 7,
            requires: vec!["acme_v1".into()],
        },
    }
}

struct Native {
    root: Root,
    identity: Identity,
    child: Mutex<fixtures::ChildGuard>,
    fail_stop: Arc<AtomicBool>,
    fault: AtomicUsize,
}
impl Native {
    fn start(base: &Path, build: &str) -> Self {
        let nonce = frontend_identity().manager_id;
        // Leave room for the socket name under Windows's account temp path.
        let root = Root::explicit(base.join(format!("w-{}", &nonce[..8]))).unwrap();
        root.prepare().unwrap();
        let child = fixtures::child(&root, &format!("worker-{build}"));
        let identity = fixtures::ready(&root);
        let mut probe = fixtures::control(&root, "stats");
        probe.manager_id = Some(identity.manager_id.clone());
        let observed = kit::client::control(&root, &probe, &fixtures::options()).unwrap();
        assert_eq!(observed.data["pid"], child.0.id());
        Self {
            root,
            identity,
            child: Mutex::new(child),
            fail_stop: Arc::new(AtomicBool::new(false)),
            fault: AtomicUsize::new(0),
        }
    }
    fn control(&self, action: &str) -> kit::Result<Value> {
        let mut message = fixtures::control(&self.root, action);
        message.manager_id = Some(self.identity.manager_id.clone());
        let reply = kit::client::control(&self.root, &message, &fixtures::options())?;
        if reply.code.is_some() {
            return Err(kit::Error::Incompatible);
        }
        Ok(reply.data)
    }
    fn call(&self, payload: Value) -> Value {
        kit::client::call(
            &self.root,
            &fixtures::request(payload),
            &fixtures::options(),
        )
        .unwrap()
        .data
    }
}
impl Worker for Native {
    fn identity(&self) -> &Identity {
        assert_ne!(
            self.fault.load(Ordering::SeqCst),
            5,
            "injected identity panic"
        );
        &self.identity
    }
    fn exited(&self) -> kit::Result<bool> {
        if self.fault.load(Ordering::SeqCst) == 7 {
            return Err(kit::Error::Worker);
        }
        Ok(self.child.lock().unwrap().0.try_wait()?.is_some())
    }
    fn prepare(&self, _: &Transition<'_>) -> kit::Result<()> {
        if self.fault.load(Ordering::SeqCst) == 6 {
            return Err(kit::Error::Worker);
        }
        assert_ne!(
            self.fault.load(Ordering::SeqCst),
            4,
            "injected prepare panic"
        );
        assert_eq!(self.control("stats")?["active"], false);
        Ok(())
    }
    fn fence(&self, _: &Transition<'_>) -> kit::Result<()> {
        if self.fault.load(Ordering::SeqCst) == 1 {
            return Err(kit::Error::Envelope);
        }
        if self.exited()? {
            return Ok(());
        }
        self.control("fence").map(|_| ())
    }
    fn retire(&self, transition: &Transition<'_>) -> kit::Result<()> {
        if self.fault.load(Ordering::SeqCst) == 3 {
            return Err(kit::Error::Envelope);
        }
        self.stop(transition.deadline)
    }
    fn activate(&self, _: &Transition<'_>) -> kit::Result<()> {
        if self.fault.load(Ordering::SeqCst) == 2 {
            return Err(kit::Error::Envelope);
        }
        self.control("activate").map(|_| ())
    }
    fn stop(&self, deadline: Instant) -> kit::Result<()> {
        if self.fail_stop.load(Ordering::SeqCst) {
            return Err(kit::Error::HandoverBusy);
        }
        let mut child = self.child.lock().unwrap();
        drop(child.0.stdin.take());
        while child.0.try_wait()?.is_none() {
            if Instant::now() >= deadline {
                return Err(kit::Error::HandoverBusy);
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        Ok(())
    }
}

struct AcmeWorker {
    active: AtomicBool,
    fenced: AtomicBool,
    build: String,
}
impl Handler for AcmeWorker {
    fn call(
        &self,
        request: &kit::protocol::Request,
        _: &Context<'_>,
    ) -> Result<Value, ProductError> {
        if !self.active.load(Ordering::SeqCst) {
            return Err(ProductError(json!("standby-or-fenced")));
        }
        Ok(json!({"value":request.payload,"pid":std::process::id(),"build":self.build}))
    }
    fn extensions(&self) -> &[&str] {
        &["activate", "fence", "stats"]
    }
    fn control(
        &self,
        request: &kit::protocol::Control,
        _: &Context<'_>,
    ) -> Result<Value, ProductError> {
        match request.action.as_str() {
            "activate" => {
                assert!(!self.fenced.load(Ordering::SeqCst));
                self.active.store(true, Ordering::SeqCst);
            }
            "fence" => {
                self.active.store(false, Ordering::SeqCst);
                self.fenced.store(true, Ordering::SeqCst);
            }
            _ => {}
        }
        Ok(json!({"active":self.active.load(Ordering::SeqCst),"pid":std::process::id()}))
    }
}
pub(super) fn entry(root: &Root, mode: &str) {
    if mode == "worker-frontend" {
        frontend(root);
        return;
    }
    let gate = Admission::new();
    let owner = gate.clone();
    // The private stdin pipe is owned only by the frontend, not the host.
    std::thread::spawn(move || {
        let _ = std::io::stdin().read(&mut [0]);
        let _ = owner.drain();
    });
    let identity = Identity::new(
        mode.strip_prefix("worker-").unwrap().into(),
        7,
        vec!["acme_v1".into()],
    )
    .unwrap();
    let handler = Arc::new(AcmeWorker {
        active: AtomicBool::new(false),
        fenced: AtomicBool::new(false),
        build: identity.build.clone(),
    });
    kit::server::serve(
        Lease::acquire(root, Duration::ZERO).unwrap(),
        identity,
        gate,
        handler,
        &Explicit,
        kit::server::Options::default(),
    )
    .unwrap();
    std::fs::write(root.path().join("exited"), b"owner-cleaned").unwrap();
}

fn supervisor(root: &Root, options: Options) -> Arc<Supervisor<Native>> {
    let lease = Lease::acquire(root, Duration::ZERO).unwrap();
    let worker = Native::start(root.path(), "1.0.0");
    worker.control("activate").unwrap();
    Arc::new(Supervisor::new(lease, frontend_identity(), worker, options).unwrap())
}

#[test]
fn worker_replacement_same_build_requires_proved_exit() {
    let (_temporary, root) = fixtures::root();
    root.prepare().unwrap();
    let supervisor = supervisor(&root, options());
    let status = supervisor.status().unwrap();
    let same = request(&status.frontend.manager_id, 1, "1.0.0");
    let dispatch = supervisor.enter().unwrap();
    let old = dispatch.worker();
    let old_id = old.identity.manager_id.clone();
    old.stop(Instant::now() + options().cleanup).unwrap();
    old.fault.store(7, Ordering::SeqCst);
    assert!(matches!(
        supervisor.replace(&same, |_| panic!("exit was not proved"), |_| Ok(())),
        Err(kit::Error::Worker)
    ));
    assert_eq!(supervisor.status().unwrap().phase, Phase::Serving);
    old.fault.store(0, Ordering::SeqCst);
    drop(dispatch);
    let replaced = supervisor.replace(
        &same,
        |_| Ok(Native::start(root.path(), "1.0.0")),
        |_| Ok(()),
    );
    assert_eq!(replaced.unwrap().phase, Phase::Replaced);
    let dispatch = supervisor.enter().unwrap();
    assert_ne!(dispatch.worker().identity.manager_id, old_id);
    assert_eq!(dispatch.worker().call(json!("recovered"))["build"], "1.0.0");
    drop(dispatch);
    supervisor.shutdown().unwrap();
}

#[test]
fn worker_replacement_waits_for_calls_and_resource_holds_without_changing_their_target() {
    let (_temporary, root) = fixtures::root();
    root.prepare().unwrap();
    let supervisor = supervisor(&root, options());
    let dispatch = supervisor.enter().unwrap();
    let hold = dispatch.hold().unwrap();
    let old = dispatch.worker().identity.manager_id.clone();
    let request = request(
        &supervisor.status().unwrap().frontend.manager_id,
        1,
        "2.0.0",
    );
    let worker = Arc::clone(&supervisor);
    let base = root.path().to_owned();
    let (sent, received) = std::sync::mpsc::channel();
    let thread = std::thread::spawn(move || {
        worker.replace(
            &request,
            |_| {
                let candidate = Native::start(&base, "2.0.0");
                sent.send(()).unwrap();
                Ok(candidate)
            },
            |_| Ok(()),
        )
    });
    received.recv_timeout(Duration::from_secs(3)).unwrap();
    assert!(matches!(supervisor.enter(), Err(kit::Error::Draining)));
    assert_eq!(dispatch.worker().call(json!("original"))["build"], "1.0.0");
    drop(dispatch);
    assert_eq!(supervisor.status().unwrap().held, 1);
    drop(hold);
    assert_eq!(thread.join().unwrap().unwrap().phase, Phase::Replaced);
    let next = supervisor.enter().unwrap();
    assert_ne!(next.worker().identity.manager_id, old);
    assert_eq!(next.worker().call(json!("new"))["build"], "2.0.0");
    drop(next);
    supervisor.shutdown().unwrap();
}

#[test]
fn worker_replacement_late_candidate_is_reaped_and_original_call_is_not_cancelled() {
    let (_temporary, root) = fixtures::root();
    root.prepare().unwrap();
    let mut limits = options();
    limits.preparation = Duration::from_millis(30);
    let supervisor = supervisor(&root, limits);
    let dispatch = supervisor.enter().unwrap();
    let request = request(
        &supervisor.status().unwrap().frontend.manager_id,
        1,
        "2.0.0",
    );
    let mut candidate_path = None;
    assert!(
        supervisor
            .replace(
                &request,
                |deadline| {
                    let candidate = Native::start(root.path(), "2.0.0");
                    candidate_path = Some(candidate.root.path().to_owned());
                    std::thread::sleep(
                        deadline.saturating_duration_since(Instant::now())
                            + Duration::from_millis(1),
                    );
                    Ok(candidate)
                },
                |_| panic!("must not commit")
            )
            .is_err()
    );
    assert!(candidate_path.unwrap().join("exited").exists());
    assert_eq!(
        dispatch.worker().call(json!("still-owned"))["build"],
        "1.0.0"
    );
    drop(dispatch);
    assert_eq!(supervisor.status().unwrap().phase, Phase::Deferred);
    assert!(supervisor.enter().is_ok());
    supervisor.shutdown().unwrap();
}

#[test]
fn worker_replacement_commit_failure_retains_both_handles_and_never_reopens_dispatch() {
    let (_temporary, root) = fixtures::root();
    root.prepare().unwrap();
    let supervisor = supervisor(&root, options());
    let request = request(
        &supervisor.status().unwrap().frontend.manager_id,
        1,
        "2.0.0",
    );
    let mut candidate_path = None;
    assert!(
        supervisor
            .replace(
                &request,
                |_| {
                    let worker = Native::start(root.path(), "2.0.0");
                    candidate_path = Some(worker.root.path().to_owned());
                    Ok(worker)
                },
                |_| Err(kit::Error::OutcomeUnknown("acme-commit".into()))
            )
            .is_err()
    );
    assert_eq!(supervisor.status().unwrap().phase, Phase::Reconcile);
    assert!(supervisor.enter().is_err());
    let candidate_path = candidate_path.unwrap();
    assert!(!candidate_path.join("exited").exists());
    let observed = supervisor
        .replace(
            &request,
            |_| panic!("replay launched"),
            |_| panic!("replay committed"),
        )
        .unwrap();
    assert_eq!(observed.phase, Phase::Reconcile);
    supervisor.shutdown().unwrap();
    assert!(candidate_path.join("exited").exists());
    assert_eq!(
        supervisor.outcome(&request).unwrap().commit,
        kit::worker::Commit::Unknown
    );
}

#[test]
fn worker_replacement_fence_retire_activate_failures_retain_responsibility() {
    for fault in [1, 2, 3] {
        let (_temporary, root) = fixtures::root();
        root.prepare().unwrap();
        let supervisor = supervisor(&root, options());
        if matches!(fault, 1 | 3) {
            let dispatch = supervisor.enter().unwrap();
            dispatch.worker().fault.store(fault, Ordering::SeqCst);
        }
        let request = request(
            &supervisor.status().unwrap().frontend.manager_id,
            1,
            "2.0.0",
        );
        let result = supervisor.replace(
            &request,
            |_| {
                let candidate = Native::start(root.path(), "2.0.0");
                if fault == 2 {
                    candidate.fault.store(fault, Ordering::SeqCst);
                }
                Ok(candidate)
            },
            |_| Ok(()),
        );
        assert!(result.is_err());
        assert_eq!(supervisor.status().unwrap().phase, Phase::Reconcile);
        assert!(supervisor.enter().is_err());
        supervisor.shutdown().unwrap();
        assert_eq!(supervisor.status().unwrap().phase, Phase::Stopped);
    }
}

#[test]
fn worker_replacement_pre_fence_panics_and_worker_errors_clean_candidate_and_defer() {
    for fault in [4, 5, 6] {
        let (_temporary, root) = fixtures::root();
        root.prepare().unwrap();
        let supervisor = supervisor(&root, options());
        let request = request(
            &supervisor.status().unwrap().frontend.manager_id,
            1,
            "2.0.0",
        );
        let mut candidate_path = None;
        assert!(matches!(
            supervisor.replace(
                &request,
                |_| {
                    let candidate = Native::start(root.path(), "2.0.0");
                    candidate_path = Some(candidate.root.path().to_owned());
                    candidate.fault.store(fault, Ordering::SeqCst);
                    Ok(candidate)
                },
                |_| panic!("failed preparation committed"),
            ),
            Err(kit::Error::Worker)
        ));
        assert!(candidate_path.unwrap().join("exited").exists());
        assert_eq!(supervisor.status().unwrap().phase, Phase::Deferred);
        let dispatch = supervisor.enter().unwrap();
        assert_eq!(dispatch.worker().call(json!("healthy"))["build"], "1.0.0");
        drop(dispatch);
        supervisor.shutdown().unwrap();
    }
}

#[test]
fn worker_replacement_host_disconnect_during_launch_is_latched_and_cleans_late_process() {
    let (_temporary, root) = fixtures::root();
    root.prepare().unwrap();
    let supervisor = supervisor(&root, options());
    let request = request(
        &supervisor.status().unwrap().frontend.manager_id,
        1,
        "2.0.0",
    );
    let (sent, received) = std::sync::mpsc::channel();
    let (resume, wait) = std::sync::mpsc::channel();
    let worker = Arc::clone(&supervisor);
    let base = root.path().to_owned();
    let thread = std::thread::spawn(move || {
        worker.replace(
            &request,
            |_| {
                sent.send(()).unwrap();
                wait.recv().unwrap();
                Ok(Native::start(&base, "2.0.0"))
            },
            |_| panic!("closed host committed"),
        )
    });
    received.recv_timeout(Duration::from_secs(3)).unwrap();
    assert_eq!(supervisor.status().unwrap().phase, Phase::Preparing);
    assert!(matches!(
        supervisor.shutdown(),
        Err(kit::Error::HandoverBusy)
    ));
    assert!(supervisor.enter().is_err());
    resume.send(()).unwrap();
    assert!(thread.join().unwrap().is_err());
    assert_eq!(supervisor.status().unwrap().phase, Phase::Stopped);
    supervisor.shutdown().unwrap();
}

#[test]
fn worker_replacement_deduplicates_changed_and_expired_attempts_and_bounds_dispatch() {
    let (_temporary, root) = fixtures::root();
    root.prepare().unwrap();
    let mut limits = options();
    limits.history = 1;
    limits.capacity = 1;
    let supervisor = supervisor(&root, limits);
    let frontend = supervisor.status().unwrap().frontend.manager_id;
    let held = supervisor.enter().unwrap();
    assert!(matches!(supervisor.enter(), Err(kit::Error::Capacity)));
    drop(held);
    let same = request(&frontend, 1, "1.0.0");
    assert!(matches!(
        supervisor.replace(&same, |_| panic!("live worker replaced"), |_| Ok(())),
        Err(kit::Error::Incompatible)
    ));
    let first = request(&frontend, 1, "2.0.0");
    assert!(
        supervisor
            .replace(
                &first,
                |_| Ok(Native::start(root.path(), "3.0.0")),
                |_| panic!("incompatible commit")
            )
            .is_err()
    );
    assert_eq!(supervisor.outcome(&first).unwrap().phase, Phase::Deferred);
    assert_eq!(
        supervisor
            .replace(
                &first,
                |_| panic!("duplicate launch"),
                |_| panic!("duplicate commit")
            )
            .unwrap()
            .phase,
        Phase::Deferred
    );
    let mut changed = first.clone();
    changed.selection.key = "another-image".into();
    assert!(
        supervisor
            .replace(&changed, |_| panic!("changed launch"), |_| Ok(()))
            .is_err()
    );
    let second = request(&frontend, 2, "2.0.0");
    supervisor
        .replace(
            &second,
            |_| Ok(Native::start(root.path(), "2.0.0")),
            |_| Ok(()),
        )
        .unwrap();
    assert!(supervisor.outcome(&first).is_err());
    assert!(
        supervisor
            .replace(&first, |_| panic!("expired launch"), |_| Ok(()))
            .is_err()
    );
    supervisor.shutdown().unwrap();
}

pub(super) fn frontend(root: &Root) {
    root.prepare().unwrap();
    let supervisor = supervisor(root, options());
    let endpoint = supervisor.lease().bind(false).unwrap();
    endpoint.listener().set_nonblocking(false).unwrap();
    let (mut stream, _) = endpoint.listener().accept().unwrap();
    kit::endpoint::check_peer(&stream).unwrap();
    kit::frame::write_json(&mut stream, &json!({"frontend":std::process::id()}), 4096).unwrap();
    while let Ok(message) = kit::frame::read_json::<Value>(&mut stream, 4096) {
        let mut reservation = None;
        let reply = if let Some(build) = message["upgrade"].as_str() {
            let request = request(
                &supervisor.status().unwrap().frontend.manager_id,
                message["sequence"].as_u64().unwrap(),
                build,
            );
            let outcome = supervisor
                .replace(
                    &request,
                    |_| Ok(Native::start(root.path(), build)),
                    |_| Ok(()),
                )
                .unwrap();
            json!({"phase":outcome.phase})
        } else {
            reservation = Some(supervisor.enter().unwrap());
            let mut result = reservation.as_ref().unwrap().worker().call(message);
            result["frontend"] = json!(std::process::id());
            result
        };
        kit::frame::write_json(&mut stream, &reply, 4096).unwrap();
        drop(reservation);
    }
    supervisor.shutdown().unwrap();
    std::fs::write(root.path().join("frontend-exited"), b"clean").unwrap();
}

#[test]
fn worker_replacement_outcome_bytes_evict_old_records_without_replaying_expired_numbers() {
    let (_temporary, root) = fixtures::root();
    root.prepare().unwrap();
    let mut limits = options();
    limits.history = 32;
    limits.history_bytes = 64 * 1024;
    let supervisor = supervisor(&root, limits);
    let frontend = supervisor.status().unwrap().frontend.manager_id;
    let first = request(&frontend, 1, "2.0.0");
    supervisor
        .replace(
            &first,
            |_| Ok(Native::start(root.path(), "2.0.0")),
            |_| Ok(()),
        )
        .unwrap();
    assert_eq!(supervisor.outcome(&first).unwrap().phase, Phase::Replaced);
    let second = request(&frontend, 2, "3.0.0");
    supervisor
        .replace(
            &second,
            |_| Ok(Native::start(root.path(), "3.0.0")),
            |_| Ok(()),
        )
        .unwrap();
    assert!(supervisor.outcome(&first).is_err());
    assert!(
        supervisor
            .replace(&first, |_| panic!("evicted request launched"), |_| Ok(()))
            .is_err()
    );
    assert_eq!(supervisor.outcome(&second).unwrap().phase, Phase::Replaced);
    supervisor.shutdown().unwrap();
}

#[test]
fn worker_replacement_candidate_stop_failure_does_not_skip_active_cleanup_on_host_loss() {
    let (_temporary, root) = fixtures::root();
    root.prepare().unwrap();
    let supervisor = supervisor(&root, options());
    let dispatch = supervisor.enter().unwrap();
    let active_path = dispatch.worker().root.path().to_owned();
    let request = request(
        &supervisor.status().unwrap().frontend.manager_id,
        1,
        "2.0.0",
    );
    let candidate = Native::start(root.path(), "2.0.0");
    let candidate_path = candidate.root.path().to_owned();
    let fail_stop = Arc::clone(&candidate.fail_stop);
    fail_stop.store(true, Ordering::SeqCst);
    let error = supervisor
        .replace(
            &request,
            |_| {
                supervisor.close();
                Ok(candidate)
            },
            |_| panic!("closed host committed"),
        )
        .unwrap_err();
    let kit::Error::WorkerCleanup { cause, cleanup } = error else {
        panic!("replacement and cleanup causes were not retained");
    };
    assert!(matches!(*cause, kit::Error::Draining));
    assert!(matches!(*cleanup, kit::Error::HandoverBusy));
    assert!(active_path.join("exited").exists());
    assert!(!candidate_path.join("exited").exists());
    let status = supervisor.status().unwrap();
    assert_eq!(status.phase, Phase::Reconcile);
    assert!(status.closing);
    assert!(status.active.is_none());
    assert!(supervisor.enter().is_err());
    // Successful cleanup is removed from retries, even if it would now fail.
    dispatch.worker().fail_stop.store(true, Ordering::SeqCst);
    assert!(matches!(
        supervisor.shutdown(),
        Err(kit::Error::HandoverBusy)
    ));
    assert!(matches!(
        Lease::acquire(&root, Duration::ZERO),
        Err(kit::Error::AlreadyRunning)
    ));
    fail_stop.store(false, Ordering::SeqCst);
    supervisor.shutdown().unwrap();
    assert!(candidate_path.join("exited").exists());
    assert_eq!(supervisor.status().unwrap().phase, Phase::Stopped);
}

#[test]
fn worker_replacement_failed_cleanup_keeps_the_exact_handle_and_domain_lease_until_retry() {
    let (_temporary, root) = fixtures::root();
    root.prepare().unwrap();
    let supervisor = supervisor(&root, options());
    let dispatch = supervisor.enter().unwrap();
    let path = dispatch.worker().root.path().to_owned();
    dispatch.worker().fail_stop.store(true, Ordering::SeqCst);
    assert!(supervisor.shutdown().is_err());
    assert_eq!(supervisor.status().unwrap().phase, Phase::Reconcile);
    assert!(matches!(
        Lease::acquire(&root, Duration::ZERO),
        Err(kit::Error::AlreadyRunning)
    ));
    dispatch.worker().fail_stop.store(false, Ordering::SeqCst);
    drop(dispatch);
    supervisor.shutdown().unwrap();
    assert!(path.join("exited").exists());
    assert_eq!(supervisor.status().unwrap().phase, Phase::Stopped);
    drop(supervisor);
    assert!(Lease::acquire(&root, Duration::ZERO).is_ok());
}

#[test]
fn worker_replacement_an_admitted_dispatch_retains_exclusion_after_controller_drop() {
    let (_temporary, root) = fixtures::root();
    root.prepare().unwrap();
    let supervisor = supervisor(&root, options());
    let dispatch = supervisor.enter().unwrap();
    drop(supervisor);
    assert!(matches!(
        Lease::acquire(&root, Duration::ZERO),
        Err(kit::Error::AlreadyRunning)
    ));
    assert_eq!(
        dispatch.worker().call(json!("already-admitted"))["build"],
        "1.0.0"
    );
    drop(dispatch);
    assert!(Lease::acquire(&root, Duration::ZERO).is_ok());
}

#[test]
fn worker_replacement_resource_holds_retain_worker_and_exclusion_after_controller_drop() {
    let (_temporary, root) = fixtures::root();
    root.prepare().unwrap();
    let supervisor = supervisor(&root, options());
    let dispatch = supervisor.enter().unwrap();
    let worker_root = dispatch.worker().root.clone();
    let hold = dispatch.hold().unwrap();
    drop(dispatch);
    assert_eq!(supervisor.status().unwrap().work, 0);
    assert_eq!(supervisor.status().unwrap().held, 1);
    drop(supervisor);
    assert!(matches!(
        Lease::acquire(&root, Duration::ZERO),
        Err(kit::Error::AlreadyRunning)
    ));
    let split = hold.split();
    drop(hold);
    let last = split.split();
    drop(split);
    assert!(matches!(
        Lease::acquire(&root, Duration::ZERO),
        Err(kit::Error::AlreadyRunning)
    ));
    assert!(matches!(
        Lease::acquire(&worker_root, Duration::ZERO),
        Err(kit::Error::AlreadyRunning)
    ));
    drop(last);
    assert!(Lease::acquire(&root, Duration::ZERO).is_ok());
    assert!(Lease::acquire(&worker_root, Duration::ZERO).is_ok());
}

#[test]
fn worker_replacement_abrupt_frontend_loss_closes_the_private_owner_channel() {
    let (_temporary, root) = fixtures::root();
    root.prepare().unwrap();
    let mut frontend = fixtures::child(&root, "worker-frontend");
    let mut stream = fixtures::wait(|| {
        kit::endpoint::connect_local(&root.path().join("manager.sock"), Duration::from_millis(50))
            .ok()
    });
    stream
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    let _: Value = kit::frame::read_json(&mut stream, 4096).unwrap();
    let worker = std::fs::read_dir(root.path())
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| {
            path.file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("w-")
        })
        .unwrap();
    frontend.0.kill().unwrap();
    frontend.0.wait().unwrap();
    fixtures::wait(|| worker.join("exited").exists().then_some(()));
    assert!(kit::frame::read_json::<Value>(&mut stream, 4096).is_err());
    let worker_root = Root::explicit(worker).unwrap();
    assert!(Lease::acquire(&worker_root, Duration::ZERO).is_ok());
}

#[test]
fn worker_replacement_stable_frontend_keeps_one_host_stream_through_successive_processes() {
    let (_temporary, root) = fixtures::root();
    root.prepare().unwrap();
    let mut frontend = fixtures::child(&root, "worker-frontend");
    let mut stream = fixtures::wait(|| {
        kit::endpoint::connect_local(&root.path().join("manager.sock"), Duration::from_millis(50))
            .ok()
    });
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    stream
        .set_write_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let hello: Value = kit::frame::read_json(&mut stream, 4096).unwrap();
    let mut previous = None;
    for (sequence, build) in [(0, "1.0.0"), (1, "2.0.0"), (2, "3.0.0")] {
        if sequence > 0 {
            kit::frame::write_json(
                &mut stream,
                &json!({"upgrade":build,"sequence":sequence}),
                4096,
            )
            .unwrap();
            let outcome: Value = kit::frame::read_json(&mut stream, 4096).unwrap();
            assert_eq!(outcome["phase"], "replaced");
        }
        kit::frame::write_json(&mut stream, &json!([0, 255, 10, 0, 123]), 4096).unwrap();
        let reply: Value = kit::frame::read_json(&mut stream, 4096).unwrap();
        assert_eq!(reply["frontend"], hello["frontend"]);
        assert_eq!(reply["value"], json!([0, 255, 10, 0, 123]));
        assert_eq!(reply["build"], build);
        assert_ne!(previous, Some(reply["pid"].clone()));
        previous = Some(reply["pid"].clone());
    }
    drop(stream);
    fixtures::wait(|| frontend.0.try_wait().unwrap());
    assert!(root.path().join("frontend-exited").exists());
}
