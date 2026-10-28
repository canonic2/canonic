//! Recovery across process death, shutdown overlap and disconnected owners.

use super::fixtures::*;
use canonic_manager::{
    Error, client,
    core::{Admission, Explicit},
    endpoint::{Lease, Root},
    frame,
    launch::Launcher,
    owner,
    protocol::{self, Code, Reply, Request},
    recovery, server,
};
use serde_json::json;
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

struct Launch {
    root: Root,
    children: Mutex<Vec<ChildGuard>>,
}
impl Launcher for Launch {
    fn start(&self) -> canonic_manager::Result<()> {
        self.children
            .lock()
            .unwrap()
            .push(child(&self.root, "manager"));
        Ok(())
    }
}
impl Launch {
    fn new(root: &Root) -> Self {
        Self {
            root: root.clone(),
            children: Mutex::new(vec![]),
        }
    }
    fn finish(&self) {
        wait(|| {
            let running = self
                .children
                .lock()
                .unwrap()
                .iter_mut()
                .filter_map(|child| child.0.try_wait().unwrap().is_none().then_some(()))
                .count();
            (running == 1).then_some(())
        });
        stop(&self.root, "drain");
        for mut child in std::mem::take(&mut *self.children.lock().unwrap()) {
            let status = child.0.wait().unwrap();
            assert!(status.success() || status.code() == Some(73));
        }
    }
}

#[test]
fn connect_or_start_waits_for_the_live_lease_before_activating() {
    let (_temp, root) = root();
    let lease = Lease::acquire(&root, Duration::ZERO).unwrap();
    let launch = Launch::new(&root);
    let mut opts = options();
    opts.readiness = Duration::from_millis(100);
    let result = client::connect(&root, Some(&launch), &opts);
    let children = std::mem::take(&mut *launch.children.lock().unwrap());
    let activations = children.len();
    // Reap premature contenders before releasing exclusion, even on failure.
    drop(children);
    assert!(root.leased().unwrap());
    drop(lease);
    assert!(matches!(result, Err(Error::Starting)));
    assert_eq!(activations, 0);
}

#[test]
fn connect_or_start_retries_dropped_and_timed_out_identity_probes_before_replacement() {
    for timeout in [false, true] {
        let (_temp, root) = root();
        let lease = Lease::acquire(&root, Duration::ZERO).unwrap();
        let endpoint = lease.bind(true).unwrap();
        let launch = Arc::new(Launch::new(&root));
        let caller_root = root.clone();
        let caller_launch = launch.clone();
        let caller = std::thread::spawn(move || {
            let mut opts = options();
            opts.io = Duration::from_millis(50);
            client::connect(&caller_root, Some(caller_launch.as_ref()), &opts)
        });
        let (mut first, _) = wait(|| endpoint.listener().accept().ok());
        first.set_nonblocking(false).unwrap();
        first
            .set_read_timeout(Some(Duration::from_secs(1)))
            .unwrap();
        let probe: protocol::Control = frame::read_json(&mut first, frame::DEFAULT_LIMIT).unwrap();
        assert_eq!(probe.action, "identity");
        // A retained silent stream exceeds io; closing it models a lost reply.
        let first = if timeout {
            Some(first)
        } else {
            drop(first);
            None
        };
        let (mut second, _) = wait(|| endpoint.listener().accept().ok());
        second.set_nonblocking(false).unwrap();
        second
            .set_read_timeout(Some(Duration::from_secs(1)))
            .unwrap();
        let retry: protocol::Control = frame::read_json(&mut second, frame::DEFAULT_LIMIT).unwrap();
        assert_eq!(retry.action, "identity");
        assert_eq!(probe.request_id, retry.request_id);
        assert!(launch.children.lock().unwrap().is_empty());
        // The old endpoint stops serving; only lease release permits activation.
        drop(first);
        drop(second);
        drop(endpoint);
        drop(lease);
        let identity = caller.join().unwrap().unwrap();
        assert_ne!(identity.manager_id, super::fixtures::identity().manager_id);
        assert_eq!(launch.children.lock().unwrap().len(), 1);
        launch.finish();
    }
}

#[test]
fn connect_or_start_waits_for_drain_then_reaches_a_fresh_process() {
    let (_temp, root) = root();
    let gate = Admission::new();
    let held = gate.hold().unwrap();
    let server = thread_server(
        &root,
        &gate,
        Arc::new(Acme::default()),
        server::Options::default(),
    );
    let old = ready(&root);
    stop(&root, "drain");
    assert!(matches!(
        client::connect(&root, None, &options()),
        Err(Error::Draining)
    ));
    let launch = Arc::new(Launch::new(&root));
    let caller_root = root.clone();
    let caller_launch = launch.clone();
    let caller = std::thread::spawn(move || {
        client::connect(&caller_root, Some(caller_launch.as_ref()), &options())
    });
    // The old owner is still excluded and the starter has no authority to replace it.
    assert!(root.leased().unwrap());
    assert_eq!(
        client::control(&root, &control(&root, "identity"), &options())
            .unwrap()
            .data["manager_id"],
        old.manager_id
    );
    drop(held);
    server.join().unwrap().unwrap();
    let current = caller.join().unwrap().unwrap();
    assert_ne!(current.manager_id, old.manager_id);
    launch.finish();
}

#[test]
fn drain_wait_is_bounded_and_never_forces_live_holds() {
    let (_temp, root) = root();
    let gate = Admission::new();
    let held = gate.hold().unwrap();
    let server = thread_server(
        &root,
        &gate,
        Arc::new(Acme::default()),
        server::Options::default(),
    );
    ready(&root);
    stop(&root, "drain");
    let launch = Launch::new(&root);
    let mut opts = options();
    opts.readiness = Duration::from_millis(100);
    assert!(matches!(
        client::connect(&root, Some(&launch), &opts),
        Err(Error::Starting)
    ));
    assert!(launch.children.lock().unwrap().is_empty());
    assert_eq!(gate.snapshot().held, 1);
    drop(held);
    server.join().unwrap().unwrap();
}

#[test]
fn force_stop_is_opt_in_incarnation_checked_and_releases_the_kernel_lease() {
    let (_temp, root) = root();
    let mut stuck = child(&root, "stuck");
    let old = ready(&root);
    assert!(old.capabilities.contains(&protocol::FORCE_STOP.to_string()));
    stop(&root, "drain");
    let mut force = control(&root, "force_stop");
    force.requires = vec![protocol::FORCE_STOP.into()];
    force.manager_id = Some("acme-stale".into());
    assert_eq!(
        client::control(&root, &force, &options()).unwrap().code,
        Some(Code::StaleManager)
    );
    assert!(root.leased().unwrap());
    force.manager_id = Some(old.manager_id.clone());
    force.arguments = json!({"unexpected":true});
    assert_eq!(
        client::control(&root, &force, &options()).unwrap().code,
        Some(Code::Invalid)
    );
    force.arguments = json!(null);
    force.requires.push("acme-missing-capability".into());
    assert!(matches!(
        client::control(&root, &force, &options()),
        Err(Error::Incompatible)
    ));
    force.requires.pop();
    assert_eq!(
        client::control(&root, &force, &options()).unwrap().data["stopping"],
        true
    );
    assert_eq!(wait(|| stuck.0.try_wait().unwrap()).code(), Some(75));
    assert!(!root.leased().unwrap());
    let launch = Launch::new(&root);
    let current = client::connect(&root, Some(&launch), &options()).unwrap();
    assert_ne!(current.manager_id, old.manager_id);
    force.manager_id = Some(current.manager_id);
    force.requires.clear();
    assert!(matches!(
        client::control(&root, &force, &options()),
        Err(Error::Incompatible)
    ));
    launch.finish();
}

#[test]
fn killed_owner_is_detected_but_cleanup_keeps_the_hold_until_explicit_release() {
    let (_temp, root) = root();
    let lease = Lease::acquire(&root, Duration::ZERO).unwrap();
    let endpoint = lease.bind(false).unwrap();
    let mut child = child(&root, "owner-client");
    let (stream, _) = wait(|| endpoint.listener().accept().ok());
    let gate = Admission::new();
    let mut owner = owner::Connection::new(stream, gate.hold().unwrap()).unwrap();
    gate.ready();
    assert!(!owner.wait(Duration::from_millis(10)).unwrap());
    assert!(gate.drain());
    child.0.kill().unwrap();
    child.0.wait().unwrap();
    assert!(owner.wait(Duration::from_secs(1)).unwrap());
    assert!(owner.wait(Duration::ZERO).unwrap());
    assert_eq!(gate.snapshot().held, 1);
    assert!(!gate.tick(Duration::from_secs(100), &Explicit));
    // Product cleanup completes before releasing the associated obligation.
    drop(owner);
    assert!(gate.tick(Duration::from_secs(101), &Explicit));
}

fn lost_result(root: &Root, acknowledge: bool) -> std::thread::JoinHandle<Request> {
    let lease = Lease::acquire(root, Duration::ZERO).unwrap();
    let (ready, bound) = std::sync::mpsc::sync_channel(0);
    let fixture = std::thread::spawn(move || {
        let ordinary = lease.bind(false).unwrap();
        let controls = lease.bind(true).unwrap();
        ready.send(()).unwrap();
        let (mut stream, _) = wait(|| controls.listener().accept().ok());
        stream.set_nonblocking(false).unwrap();
        let request: protocol::Control =
            frame::read_json(&mut stream, frame::DEFAULT_LIMIT).unwrap();
        let mut reply = Reply {
            control_version: 1,
            manager_id: identity().manager_id,
            request_id: request.request_id,
            accepted: Some(true),
            code: None,
            data: json!(null),
        };
        frame::write_json(&mut stream, &reply, frame::DEFAULT_LIMIT).unwrap();
        reply.accepted = None;
        reply.data = json!(identity());
        frame::write_json(&mut stream, &reply, frame::DEFAULT_LIMIT).unwrap();
        let (mut stream, _) = wait(|| ordinary.listener().accept().ok());
        stream.set_nonblocking(false).unwrap();
        let request: Request = frame::read_json(&mut stream, frame::DEFAULT_LIMIT).unwrap();
        if acknowledge {
            reply.accepted = Some(true);
            reply.request_id.clone_from(&request.request_id);
            reply.data = json!(null);
            frame::write_json(&mut stream, &reply, frame::DEFAULT_LIMIT).unwrap();
        }
        // Simulate a crash after receiving work: discard the result and release exclusion.
        drop(ordinary);
        drop(controls);
        drop(lease);
        request
    });
    bound.recv().unwrap();
    fixture
}

#[test]
fn recovery_replays_the_identical_request_only_after_product_authorization() {
    for acknowledge in [false, true] {
        let (_temp, root) = root();
        let crashed = lost_result(&root, acknowledge);
        let launch = Launch::new(&root);
        let original = request(json!({"effect":"acme"}));
        let mut checked = 0;
        let reply = recovery::call(
            &root,
            &launch,
            &original,
            &options(),
            &recovery::Options {
                attempts: 2,
                timeout: Duration::from_secs(5),
            },
            |received, _| {
                assert_eq!(received.request_id, original.request_id);
                assert_eq!(received.payload, original.payload);
                checked += 1;
                Ok(recovery::Decision::Replay)
            },
        )
        .unwrap();
        let sent = crashed.join().unwrap();
        assert_eq!(sent.request_id, reply.request_id);
        assert_eq!(sent.payload, reply.data);
        assert_eq!(checked, 1);
        launch.finish();
    }
}

#[test]
fn recovery_can_return_a_durable_result_without_replaying_or_refuse_unsafe_replay() {
    for decision in 0..4 {
        let (_temp, root) = root();
        let crashed = lost_result(&root, true);
        let launch = Launch::new(&root);
        let request = request(json!("acme-effect"));
        let result = recovery::call(
            &root,
            &launch,
            &request,
            &options(),
            &recovery::Options {
                attempts: 1,
                timeout: Duration::from_secs(5),
            },
            |r, identity| {
                Ok(match decision {
                    0 => recovery::Decision::Unknown,
                    1 | 3 => recovery::Decision::Recovered(Reply {
                        control_version: 1,
                        manager_id: identity.manager_id.clone(),
                        request_id: if decision == 3 {
                            "acme-wrong-request".into()
                        } else {
                            r.request_id.clone()
                        },
                        accepted: None,
                        code: None,
                        data: json!("recorded-result"),
                    }),
                    _ => recovery::Decision::Replay,
                })
            },
        );
        crashed.join().unwrap();
        if decision == 1 {
            assert_eq!(result.unwrap().data, "recorded-result");
        } else if decision == 3 {
            assert!(matches!(result, Err(Error::Envelope)));
        } else {
            assert!(matches!(result, Err(Error::OutcomeUnknown(id)) if id == request.request_id));
        }
        // A recovered result or unresolved outcome does not dispatch on the successor.
        assert_eq!(
            client::call(
                &root,
                &super::fixtures::request(json!("dispatch-count")),
                &options()
            )
            .unwrap()
            .data,
            0
        );
        launch.finish();
    }
}

#[test]
fn owner_channel_data_is_not_death_and_invalid_recovery_bounds_start_nothing() {
    use std::io::Write;
    let gate = Admission::new();
    let (stream, mut peer) = std::os::unix::net::UnixStream::pair().unwrap();
    let mut owner = owner::Connection::new(stream, gate.hold().unwrap()).unwrap();
    peer.write_all(b"unexpected").unwrap();
    assert!(matches!(
        owner.wait(Duration::from_secs(1)),
        Err(Error::Envelope)
    ));
    assert_eq!(gate.snapshot().held, 1);
    let (_temp, root) = root();
    let launch = Launch::new(&root);
    for (attempts, timeout) in [(0, Duration::from_secs(1)), (1, Duration::ZERO)] {
        assert!(matches!(
            recovery::call(
                &root,
                &launch,
                &request(json!(null)),
                &options(),
                &recovery::Options { attempts, timeout },
                |_, _| panic!("invalid options must not reconcile")
            ),
            Err(Error::Options)
        ));
    }
    assert!(!root.path().exists());
    assert!(launch.children.lock().unwrap().is_empty());
}

#[test]
fn recovery_after_real_manager_death_reads_the_durable_outcome_without_duplicate_dispatch() {
    let (_temp, root) = root();
    let mut crashed = child(&root, "crash-after-effect");
    ready(&root);
    let launch = Launch::new(&root);
    let request = request(json!({"effect":"persisted-before-crash"}));
    let reply = recovery::call(
        &root,
        &launch,
        &request,
        &options(),
        &recovery::Options {
            attempts: 2,
            timeout: Duration::from_secs(5),
        },
        |request, identity| {
            let recorded: Request =
                serde_json::from_slice(&std::fs::read(root.path().join("acme-outcome.json"))?)
                    .unwrap();
            assert_eq!(recorded.request_id, request.request_id);
            assert_eq!(recorded.payload, request.payload);
            Ok(recovery::Decision::Recovered(Reply {
                control_version: 1,
                manager_id: identity.manager_id.clone(),
                request_id: request.request_id.clone(),
                accepted: None,
                code: None,
                data: json!("durable-success"),
            }))
        },
    )
    .unwrap();
    assert_eq!(reply.data, "durable-success");
    assert_eq!(crashed.0.wait().unwrap().code(), Some(74));
    assert_eq!(
        client::call(
            &root,
            &super::fixtures::request(json!("dispatch-count")),
            &options()
        )
        .unwrap()
        .data,
        0
    );
    launch.finish();
}
