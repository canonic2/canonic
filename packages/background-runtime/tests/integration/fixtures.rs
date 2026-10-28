//! Test-owned manager and child-process helpers; no product executable is used.

use canonic_background_runtime::{
    client,
    core::{Admission, Explicit},
    endpoint::{Lease, Root},
    launch::Spawn,
    protocol::{Control, Identity, Request},
    server::{self, Context, Handler},
};
use serde_json::{Value, json};
use std::{
    fs::OpenOptions,
    io::Read,
    os::unix::process::CommandExt,
    path::Path,
    process::{Child, Command, Stdio},
    sync::{Arc, Condvar, Mutex},
    time::Duration,
};

pub(super) fn identity() -> Identity {
    Identity {
        manager_id: format!("acme-{}", std::process::id()),
        build: "1.0.0".into(),
        protocol: 7,
        control_version: 1,
        capabilities: vec!["acme_v1".into()],
    }
}
pub(super) fn root() -> (tempfile::TempDir, Root) {
    let temp = tempfile::Builder::new()
        .prefix("acme-")
        .tempdir_in("/tmp")
        .unwrap();
    let root = Root::explicit(temp.path().join("run")).unwrap();
    (temp, root)
}
pub(super) fn control(_root: &Root, action: &str) -> Control {
    Control {
        control_version: 1,
        request_id: "acme-control".into(),
        action: action.into(),
        requires: vec![],
        manager_id: Some(identity().manager_id),
        arguments: Value::Null,
    }
}
pub(super) fn request(payload: Value) -> Request {
    Request {
        protocol: 7,
        request_id: "acme-request".into(),
        requires: vec!["acme_v1".into()],
        payload,
    }
}
pub(super) fn options() -> client::Options {
    client::Options {
        readiness: Duration::from_secs(5),
        io: Duration::from_secs(2),
        ..Default::default()
    }
}
pub(super) fn ready(root: &Root) -> Identity {
    wait(|| client::connect(root, None, &options()).ok())
}
#[track_caller]
pub(super) fn wait<T>(mut test: impl FnMut() -> Option<T>) -> T {
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    let mut delay = Duration::from_millis(1);
    loop {
        if let Some(value) = test() {
            return value;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "fixture deadline expired"
        );
        std::thread::sleep(delay);
        delay = (delay * 2).min(Duration::from_millis(20));
    }
}

#[derive(Default)]
pub(super) struct Acme {
    pub dispatched: std::sync::atomic::AtomicUsize,
    pub blocked: Mutex<bool>,
    pub changed: Condvar,
    pub entered: Mutex<usize>,
}
impl Handler for Acme {
    fn call(
        &self,
        r: &Request,
        _: &Context<'_>,
    ) -> std::result::Result<Value, canonic_background_runtime::protocol::ProductError> {
        self.dispatched
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        if r.payload == "dispatch-count" {
            return Ok(json!(
                self.dispatched.load(std::sync::atomic::Ordering::SeqCst) - 1
            ));
        }
        if r.payload == "large" {
            return Ok(json!("x".repeat(2048)));
        }
        if r.payload == "block" {
            *self.entered.lock().unwrap() += 1;
            self.changed.notify_all();
            let mut blocked = self.blocked.lock().unwrap();
            while *blocked {
                blocked = self.changed.wait(blocked).unwrap();
            }
        }
        if r.payload == "environment" {
            let stdin = std::io::stdin().read(&mut [0]).unwrap();
            return Ok(
                json!({"env":std::env::vars().collect::<std::collections::BTreeMap<_,_>>(),"stdin":stdin,"pid":std::process::id(),"sid":nix::unistd::getsid(None).unwrap().as_raw(),"group":nix::unistd::getpgrp().as_raw()}),
            );
        }
        Ok(r.payload.clone())
    }
    fn observer(&self, r: &Request) -> bool {
        r.payload == "inspect"
    }
    fn extensions(&self) -> &[&str] {
        &["acme_release"]
    }
    fn blockers(&self) -> Value {
        json!({"items":["acme-resource"]})
    }
    fn control(
        &self,
        _: &Control,
        _: &Context<'_>,
    ) -> std::result::Result<Value, canonic_background_runtime::protocol::ProductError> {
        *self.blocked.lock().unwrap() = false;
        self.changed.notify_all();
        Ok(Value::Null)
    }
}

struct Crash(std::path::PathBuf);
impl Handler for Crash {
    fn call(
        &self,
        request: &Request,
        _: &Context<'_>,
    ) -> std::result::Result<Value, canonic_background_runtime::protocol::ProductError> {
        std::fs::write(
            self.0.join("acme-outcome.json"),
            serde_json::to_vec(request).unwrap(),
        )
        .unwrap();
        std::process::exit(74);
    }
}

pub(super) fn thread_server(
    root: &Root,
    gate: &Admission,
    handler: Arc<Acme>,
    options: server::Options,
) -> std::thread::JoinHandle<canonic_background_runtime::Result<()>> {
    let lease = Lease::acquire(root, Duration::ZERO).unwrap();
    let gate = gate.clone();
    std::thread::spawn(move || server::serve(lease, identity(), gate, handler, &Explicit, options))
}

pub(super) struct ChildGuard(pub Child);
impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
pub(super) fn child(root: &Root, mode: &str) -> ChildGuard {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args(["--exact", "fixtures::entry", "--nocapture"])
        .env("ACME_ROOT", root.path())
        .env("ACME_MODE", mode)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::inherit());
    command.process_group(0);
    ChildGuard(command.spawn().unwrap())
}
pub(super) fn spawn(root: &Root) -> Spawn {
    root.prepare().unwrap();
    Spawn {
        args: vec![
            "--exact".into(),
            "fixtures::entry".into(),
            "--nocapture".into(),
        ],
        environment: vec![
            ("ACME_ROOT".into(), root.path().into()),
            ("ACME_MODE".into(), "manager".into()),
        ],
        directory: root.path().into(),
        log: OpenOptions::new()
            .create(true)
            .append(true)
            .open(root.path().join("acme.log"))
            .unwrap(),
    }
}
pub(super) fn stop(root: &Root, action: &str) {
    let id = ready(root);
    let mut request = control(root, action);
    request.manager_id = Some(id.manager_id);
    wait(|| {
        let reply = client::control(root, &request, &options()).unwrap();
        (action != "retire_if_empty" || reply.data["retired"] == true).then_some(())
    });
}

#[test]
fn entry() {
    let Some(path) = std::env::var_os("ACME_ROOT") else {
        return;
    };
    let root = Root::explicit(Path::new(&path)).unwrap();
    let mode = std::env::var("ACME_MODE").unwrap();
    match mode.as_str() {
        "owner-client" => {
            let _connection =
                canonic_background_runtime::endpoint::connect(&root, false, Duration::from_secs(2))
                    .unwrap();
            loop {
                std::thread::park();
            }
        }
        "home" => {
            let actual = Root::for_user(
                Path::new(".acme"),
                canonic_background_runtime::endpoint::Home::Account,
            )
            .unwrap();
            let expected = nix::unistd::User::from_uid(nix::unistd::geteuid())
                .unwrap()
                .unwrap()
                .dir
                .join(".acme");
            assert_eq!(actual.path(), expected);
            assert_ne!(
                actual.path(),
                Path::new(&std::env::var_os("HOME").unwrap()).join(".acme")
            );
        }
        "parent" => {
            let _reaper = spawn(&root).start_reaped().unwrap();
            ready(&root);
            let _ = std::io::stdin().read(&mut [0]);
        }
        _ => {
            if mode == "delayed-manager" {
                std::io::stdin().read_exact(&mut [0]).unwrap();
            }
            let wait = if mode == "waiting" {
                assert!(matches!(
                    Lease::acquire(&root, Duration::ZERO),
                    Err(canonic_background_runtime::Error::AlreadyRunning)
                ));
                std::fs::write(root.path().join("acme-waiting"), b"lease-observed-busy").unwrap();
                Duration::from_secs(5)
            } else {
                Duration::from_millis(200)
            };
            let lease = match Lease::acquire(&root, wait) {
                Ok(lease) => lease,
                Err(canonic_background_runtime::Error::AlreadyRunning) => std::process::exit(73),
                Err(error) => panic!("{error}"),
            };
            let mut identity = identity();
            if mode == "manager-v2" {
                identity.build = "2.0.0".into();
            }
            let gate = Admission::new();
            let _stuck = (mode == "stuck").then(|| gate.hold().unwrap());
            if mode == "crash-after-effect" {
                server::serve(
                    lease,
                    identity,
                    gate,
                    Arc::new(Crash(root.path().into())),
                    &Explicit,
                    server::Options::default(),
                )
                .unwrap();
                return;
            }
            server::serve(
                lease,
                identity,
                gate,
                Arc::new(Acme::default()),
                &Explicit,
                server::Options {
                    allow_force_stop: mode == "stuck",
                    ..Default::default()
                },
            )
            .unwrap();
        }
    }
}
