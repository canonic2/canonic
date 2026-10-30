//! Exercise the blocking-accept strategy on Unix sockets, independently of Windows ACL support.

use super::*;
use crate::{
    client,
    connection::Slots,
    core::Admission,
    endpoint::{Lease, Root},
    frame,
    protocol::{Code, Control, Identity, ProductError, Reply, Request},
    server::{Context, Options},
};
use serde_json::Value;
use std::{
    io::Read,
    sync::{Mutex, atomic::AtomicUsize},
    time::Duration,
};

const WAIT: Duration = Duration::from_secs(5);

struct Unused;
impl Handler for Unused {
    fn call(&self, _: &Request, _: &Context<'_>) -> std::result::Result<Value, ProductError> {
        panic!("silent connections must never dispatch");
    }
}

struct PausedPolicy {
    entered: mpsc::SyncSender<()>,
    release: Mutex<mpsc::Receiver<()>>,
    first: AtomicBool,
}
impl ExitPolicy for PausedPolicy {
    fn exit(&self, _: Duration) -> bool {
        if self.first.swap(false, Ordering::SeqCst) {
            self.entered.send(()).expect("report paused policy");
            let _ = self.release.lock().expect("policy release lock").recv();
        }
        false
    }
    fn next_check(&self, _: Duration) -> Option<Duration> {
        None
    }
}

struct Running {
    shared: Arc<Shared<Unused>>,
    release: mpsc::SyncSender<()>,
    thread: Option<thread::JoinHandle<Result<()>>>,
}
impl Running {
    fn start(root: &Root) -> (Self, mpsc::Receiver<()>) {
        let lease = Lease::acquire(root, Duration::ZERO).expect("acquire fixture lease");
        let shared = Arc::new(Shared {
            identity: Identity::new("1.0.0".into(), 1, vec![]).expect("fixture identity"),
            version: None,
            gate: Admission::new(),
            handler: Arc::new(Unused),
            options: Options {
                workers: 1,
                capacity: 1,
                control_workers: 1,
                control_capacity: 1,
                io_timeout: Duration::from_secs(30),
                ..Options::default()
            },
            ordinary_count: Arc::new(AtomicUsize::new(0)),
            control_count: Arc::new(AtomicUsize::new(0)),
            connections: Slots::new(0),
        });
        let (entered, paused) = mpsc::sync_channel(1);
        let (release, resume) = mpsc::sync_channel(1);
        let policy = PausedPolicy {
            entered,
            release: Mutex::new(resume),
            first: AtomicBool::new(true),
        };
        let serving = Arc::clone(&shared);
        let thread = thread::spawn(move || {
            let ordinary = lease.bind(false)?;
            let control = lease.bind(true)?;
            let mut work = Pool::new(&serving, false);
            let mut controls = Pool::new(&serving, true);
            let result = run(
                &serving,
                &policy,
                [(&control, &controls, true), (&ordinary, &work, false)],
                Instant::now(),
            );
            let joined = work.close().and(controls.close());
            serving.gate.set_waker(None);
            result.and(joined)
        });
        (
            Self {
                shared,
                release,
                thread: Some(thread),
            },
            paused,
        )
    }
}
impl Drop for Running {
    fn drop(&mut self) {
        let _ = self.release.try_send(());
        let _ = self.shared.gate.drain();
        let result = self.thread.take().expect("owned server thread").join();
        if !thread::panicking() {
            result
                .expect("server thread panicked")
                .expect("server failed");
        }
    }
}

#[test]
fn unread_accepts_are_bounded_and_controls_progress_while_driver_is_paused() {
    let temp = tempfile::Builder::new()
        .prefix("acme-")
        .tempdir_in("/tmp")
        .unwrap();
    let root = Root::explicit(temp.path().join("run")).unwrap();
    let (running, paused) = Running::start(&root);
    paused.recv_timeout(WAIT).unwrap();

    // One silent connection consumes the entire ordinary budget even while
    // the lifetime driver is blocked in its policy callback.
    let unread = Stream::connect(root.socket(false)).unwrap();
    let deadline = Instant::now() + WAIT;
    while running.shared.ordinary_count.load(Ordering::SeqCst) != 1 {
        assert!(Instant::now() < deadline, "accept did not reserve capacity");
        thread::sleep(Duration::from_millis(1));
    }
    // Retain clients without reading while both accepts and excess refusals run.
    let mut excess = Vec::new();
    for _ in 0..32 {
        let stream = Stream::connect(root.socket(false)).unwrap();
        stream.set_read_timeout(Some(WAIT)).unwrap();
        excess.push(stream);
    }
    let status = client::control(
        &root,
        &Control {
            control_version: 1,
            request_id: "acme-status".into(),
            action: "status".into(),
            requires: vec![],
            manager_id: None,
            arguments: Value::Null,
        },
        &client::Options {
            io: WAIT,
            ..client::Options::default()
        },
    )
    .unwrap();
    assert_eq!(status.data["connections"]["ordinary"], 1);
    for mut stream in excess {
        let reply: Reply = frame::read_json(&mut stream, frame::DEFAULT_LIMIT).unwrap();
        assert_eq!(reply.code, Some(Code::Capacity));
        assert_eq!(reply.accepted, Some(false));
        assert_eq!(stream.read(&mut [0]).unwrap(), 0);
    }
    drop(unread);
    drop(running);
}
