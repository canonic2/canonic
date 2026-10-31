//! Socket privacy, bounded resources, controls and lease-order acceptance.

use super::fixtures::*;
use canonic_process_kit::{
    Error, client,
    core::Admission,
    endpoint::{self, Lease},
    frame,
    net::{Listener, Stream},
    protocol::{Code, Reply},
    server,
};
use serde_json::{Value, json};
use std::{io::Write, sync::Arc, time::Duration};
#[cfg(unix)]
use std::{
    os::unix::fs::{MetadataExt, PermissionsExt},
    path::Path,
};

/// A private socket at `path`, left behind when dropped.
#[cfg(unix)]
fn foreign_listener(path: &Path) -> Listener {
    let listener = Listener::bind(path).unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).unwrap();
    listener
}

/// The live endpoint still answers: replacement attempts left it in place.
fn still_bound(root: &endpoint::Root, endpoint: &endpoint::Endpoint<'_>) {
    let connection = endpoint::connect(root, false, Duration::from_secs(1)).unwrap();
    wait(|| endpoint.listener().accept().ok());
    drop(connection);
}

#[test]
#[cfg(target_os = "macos")]
fn macos_backlog_and_stale_socket_refusals_share_the_starting_outcome() {
    use nix::sys::socket::{Backlog, listen};
    use std::os::unix::net::UnixStream;

    let (_temp, root) = root();
    let lease = Lease::acquire(&root, Duration::ZERO).unwrap();
    let endpoint = lease.bind(false).unwrap();
    listen(endpoint.listener(), Backlog::new(1).unwrap()).unwrap();
    let inode = std::fs::metadata(root.socket(false)).unwrap().ino();
    let mut queued = vec![];
    let mut backlog_error = None;
    for _ in 0..8 {
        match UnixStream::connect(root.socket(false)) {
            Ok(stream) => queued.push(stream),
            Err(error) => {
                backlog_error = Some(error);
                break;
            }
        }
    }
    let backlog_error = backlog_error.expect("small backlog did not fill");
    assert_eq!(backlog_error.kind(), std::io::ErrorKind::ConnectionRefused);
    assert!(matches!(
        endpoint::connect(&root, false, Duration::from_secs(1)),
        Err(Error::Starting)
    ));
    assert_eq!(std::fs::metadata(root.socket(false)).unwrap().ino(), inode);
    let (accepted, _) = endpoint.listener().accept().unwrap();
    drop(accepted);
    // Capacity release restores connectivity without replacing the listener.
    let connection = endpoint::connect(&root, false, Duration::from_secs(1)).unwrap();
    drop(connection);
    drop(queued);
    drop(endpoint);
    drop(foreign_listener(&root.socket(false)));
    let stale_error = UnixStream::connect(root.socket(false)).unwrap_err();
    assert_eq!(stale_error.raw_os_error(), backlog_error.raw_os_error());
    assert!(matches!(
        endpoint::connect(&root, false, Duration::from_secs(1)),
        Err(Error::Starting)
    ));
    assert!(root.leased().unwrap());
    drop(lease);
    assert!(matches!(
        endpoint::connect(&root, false, Duration::from_secs(1)),
        Err(Error::Absent)
    ));
}

#[test]
fn malformed_json_and_envelopes_receive_invalid_refusals_without_product_dispatch() {
    let (_temp, root) = root();
    let gate = Admission::new();
    let handler = Arc::new(Acme::default());
    *handler.blocked.lock().unwrap() = true;
    let server = thread_server(&root, &gate, handler.clone(), server::Options::default());
    let identity = ready(&root);
    let mut replies = vec![];
    for control_lane in [false, true] {
        for (bytes, expected_id) in invalid_frames(control_lane) {
            let mut stream =
                endpoint::connect(&root, control_lane, Duration::from_secs(1)).unwrap();
            frame::write(&mut stream, &bytes, frame::DEFAULT_LIMIT).unwrap();
            let reply = frame::read_json::<Reply>(&mut stream, frame::DEFAULT_LIMIT);
            replies.push((reply, expected_id));
        }
    }
    let dispatched = handler.dispatched.load(std::sync::atomic::Ordering::SeqCst);
    let blocked = *handler.blocked.lock().unwrap();
    stop(&root, "drain");
    server.join().unwrap().unwrap();
    for (reply, expected_id) in replies {
        let reply = reply.unwrap();
        assert_eq!(reply.accepted, Some(false));
        assert_eq!(reply.code, Some(Code::Invalid));
        assert_eq!(reply.request_id, expected_id);
        assert_eq!(reply.manager_id, identity.manager_id);
        assert_eq!(reply.control_version, 1);
        assert!(reply.data.is_null());
    }
    assert_eq!(dispatched, 0);
    assert!(
        blocked,
        "invalid control envelope reached the product callback"
    );
}

fn invalid_frames(control_lane: bool) -> Vec<(Vec<u8>, &'static str)> {
    let mut frames = vec![(b"{".to_vec(), ""), (vec![0xff], "")];
    let mut envelopes = vec![
        (
            json!({"control_version":1,"request_id":"acme-invalid","action":"acme_release","extra":true}),
            "acme-invalid",
        ),
        (
            json!({"control_version":1,"request_id":"acme-invalid"}),
            "acme-invalid",
        ),
        (
            json!({"control_version":"1","request_id":"acme-invalid","action":"acme_release"}),
            "acme-invalid",
        ),
        (
            json!({"control_version":1,"request_id":123,"action":"acme_release"}),
            "",
        ),
    ];
    if !control_lane {
        envelopes.extend([
            (
                json!({"protocol":7,"request_id":"acme-invalid","payload":"acme","extra":true}),
                "acme-invalid",
            ),
            (
                json!({"protocol":"7","request_id":"acme-invalid"}),
                "acme-invalid",
            ),
            (json!({"request_id":"acme-invalid"}), "acme-invalid"),
            (json!({"protocol":7}), ""),
            (json!({"protocol":7,"request_id":"bad id","extra":true}), ""),
            (
                json!({"protocol":7,"request_id":"x".repeat(129),"extra":true}),
                "",
            ),
        ]);
    }
    frames.extend(
        envelopes
            .into_iter()
            .map(|(value, id)| (serde_json::to_vec(&value).unwrap(), id)),
    );
    frames
}

#[test]
#[cfg(unix)]
fn real_socket_checks_peer_and_forged_other_uid_is_refused() {
    let (_temp, root) = root();
    let lease = Lease::acquire(&root, Duration::ZERO).unwrap();
    let endpoint = lease.bind(false).unwrap();
    assert_eq!(
        std::fs::metadata(root.path()).unwrap().mode() & 0o777,
        0o700
    );
    assert_eq!(
        std::fs::metadata(root.socket(false)).unwrap().mode() & 0o777,
        0o600
    );
    let client = endpoint::connect(&root, false, Duration::from_secs(1)).unwrap();
    let (server, _) = endpoint.listener().accept().unwrap();
    endpoint::check_peer(&server).unwrap();
    endpoint::check_peer(&client).unwrap();
    let uid = nix::unistd::geteuid().as_raw();
    assert!(matches!(
        endpoint::check_uid(uid.wrapping_add(1), uid),
        Err(Error::Peer)
    ));
}

#[test]
fn absent_inspection_creates_nothing_and_long_paths_are_refused() {
    let (temp, root) = root();
    assert!(matches!(
        client::connect(&root, None, &options()),
        Err(Error::Absent)
    ));
    assert!(!root.path().exists());
    assert_eq!(std::fs::read_dir(temp.path()).unwrap().count(), 0);
    assert!(matches!(
        endpoint::Root::explicit(temp.path().join("a".repeat(110))),
        Err(Error::PathTooLong)
    ));
}

#[test]
#[cfg(unix)]
fn private_roots_refuse_symlinks_and_permissive_directories() {
    let (temp, root) = root();
    root.prepare().unwrap();
    std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o755)).unwrap();
    assert!(matches!(root.prepare(), Err(Error::PrivatePath)));
    let path = temp.path().join("link");
    std::os::unix::fs::symlink(root.path(), &path).unwrap();
    assert!(matches!(
        endpoint::Root::explicit(&path).unwrap().prepare(),
        Err(Error::PrivatePath)
    ));
}

#[test]
#[cfg(unix)]
fn owned_socket_permissions_in_progress_require_a_live_lease_and_never_connect() {
    let (_temp, root) = root();
    let lease = Lease::acquire(&root, Duration::ZERO).unwrap();
    let listener = std::os::unix::net::UnixListener::bind(root.socket(true)).unwrap();
    listener.set_nonblocking(true).unwrap();
    std::fs::set_permissions(root.socket(true), std::fs::Permissions::from_mode(0o777)).unwrap();
    assert!(matches!(
        endpoint::connect(&root, true, Duration::from_secs(1)),
        Err(Error::Starting)
    ));
    assert!(listener.accept().is_err());
    drop(lease);
    assert!(matches!(
        endpoint::connect(&root, true, Duration::from_secs(1)),
        Err(Error::PrivatePath)
    ));
    assert!(listener.accept().is_err());
}

#[test]
fn stale_socket_replacement_and_live_lease_preservation() {
    let (_temp, root) = root();
    root.prepare().unwrap();
    // A plain bind on Windows gets the creator's default owner, which is the
    // Administrators group when elevated, so the library rightly refuses it.
    // The killed-manager test covers stale sockets a manager restricted.
    #[cfg(unix)]
    drop(foreign_listener(&root.socket(false)));
    let lease = Lease::acquire(&root, Duration::ZERO).unwrap();
    let endpoint = lease.bind(false).unwrap();
    #[cfg(unix)]
    let inode = std::fs::metadata(root.socket(false)).unwrap().ino();
    assert!(matches!(lease.bind(false), Err(Error::EndpointBound)));
    assert!(matches!(
        Lease::acquire(&root, Duration::ZERO),
        Err(Error::AlreadyRunning)
    ));
    #[cfg(unix)]
    assert_eq!(std::fs::metadata(root.socket(false)).unwrap().ino(), inode);
    still_bound(&root, &endpoint);
    endpoint.unlink().unwrap();
    assert!(root.leased().unwrap());
    assert!(matches!(
        client::connect(&root, None, &options()),
        Err(Error::Starting)
    ));
    drop(endpoint);
    drop(lease);
    assert!(!root.leased().unwrap());
}

#[test]
fn cleanup_preserves_a_replaced_socket_identity() {
    let (_temp, root) = root();
    let lease = Lease::acquire(&root, Duration::ZERO).unwrap();
    let endpoint = lease.bind(false).unwrap();
    std::fs::remove_file(root.socket(false)).unwrap();
    let replacement = Listener::bind(root.socket(false)).unwrap();
    endpoint.unlink().unwrap();
    drop(endpoint);
    assert!(root.socket(false).exists());
    drop(replacement);
}

#[test]
fn connections_to_a_listener_that_never_accepts_end_at_their_deadlines() {
    let (_temp, root) = root();
    let lease = Lease::acquire(&root, Duration::ZERO).unwrap();
    let _endpoint = lease.bind(true).unwrap();
    let bound = Duration::from_secs(2);
    let mut queued = vec![];
    let mut unfinished = 0;
    // Fill the backlog: later attempts are refused or never complete.
    while unfinished < 8 && queued.len() < 20_000 {
        let started = std::time::Instant::now();
        let result = endpoint::connect(&root, true, Duration::from_millis(100));
        assert!(started.elapsed() < bound, "{result:?}");
        match result {
            Ok(stream) => queued.push(stream),
            Err(Error::Starting | Error::Capacity) => unfinished += 1,
            Err(error) => panic!("{error}"),
        }
    }
    assert!(
        unfinished > 0,
        "the backlog never filled after {} connections",
        queued.len()
    );
    let started = std::time::Instant::now();
    let probe = client::connect(
        &root,
        None,
        &client::Options {
            readiness: Duration::from_millis(100),
            ..options()
        },
    );
    assert!(started.elapsed() < bound, "{probe:?}");
    assert!(
        matches!(probe, Err(Error::Starting | Error::Capacity)),
        "{probe:?}"
    );
}

#[test]
fn frames_refuse_oversize_and_partial_frames_without_truncation() {
    let mut output = vec![];
    assert!(matches!(
        frame::write(&mut output, b"acme", 3),
        Err(Error::FrameTooLarge)
    ));
    assert_eq!(output, [] as [u8; 0]);
    let mut prefix = &5_u32.to_be_bytes()[..];
    assert!(matches!(
        frame::read(&mut prefix, 4),
        Err(Error::FrameTooLarge)
    ));
    let mut bytes = vec![];
    frame::write(&mut bytes, b"acme", 4).unwrap();
    assert_eq!(frame::read(&mut &bytes[..], 4).unwrap(), b"acme");
    assert!(frame::read(&mut &bytes[..5], 4).is_err());
}

#[test]
fn queued_frame_budget_starts_when_a_worker_takes_the_connection() {
    let (_temp, root) = root();
    let gate = Admission::new();
    let handler = Arc::new(Acme::default());
    *handler.blocked.lock().unwrap() = true;
    let io_timeout = Duration::from_millis(100);
    let server = thread_server(
        &root,
        &gate,
        handler.clone(),
        server::Options {
            workers: 1,
            capacity: 2,
            io_timeout,
            ..Default::default()
        },
    );
    ready(&root);
    let path = root.clone();
    let first =
        std::thread::spawn(move || client::call(&path, &request(json!("block")), &options()));
    wait(|| (*handler.entered.lock().unwrap() == 1).then_some(()));
    let path = root.clone();
    let (completed, completion) = std::sync::mpsc::channel();
    let queued = std::thread::spawn(move || {
        completed
            .send(client::call(&path, &request(json!("queued")), &options()))
            .unwrap();
    });
    wait(|| (gate.snapshot().work == 2).then_some(()));
    // The queued request must survive more than a full frame budget while
    // its worker is occupied. No callback can complete it before release.
    let early = completion.recv_timeout(io_timeout * 2);
    *handler.blocked.lock().unwrap() = false;
    handler.changed.notify_all();
    let result = early.unwrap_or_else(|_| completion.recv_timeout(Duration::from_secs(5)).unwrap());
    let first = first.join().unwrap();
    queued.join().unwrap();
    stop(&root, "drain");
    server.join().unwrap().unwrap();
    assert_eq!(first.unwrap().data, "block");
    assert_eq!(result.unwrap().data, "queued");
}

#[test]
#[cfg(windows)]
fn windows_arrivals_during_the_exit_decision_are_served_and_postpone_exit() {
    use canonic_process_kit::core::ExitPolicy;
    use std::sync::{
        Mutex,
        atomic::{AtomicUsize, Ordering},
        mpsc,
    };

    /// Holds only the first decision open; later decisions exit at once.
    struct Deciding {
        entered: mpsc::Sender<()>,
        release: Mutex<mpsc::Receiver<()>>,
        decisions: AtomicUsize,
    }
    impl ExitPolicy for Deciding {
        fn exit(&self, _: Duration) -> bool {
            if self.decisions.fetch_add(1, Ordering::SeqCst) == 0 {
                self.entered.send(()).unwrap();
                self.release
                    .lock()
                    .unwrap()
                    .recv_timeout(Duration::from_secs(5))
                    .unwrap();
            }
            true
        }
        fn next_check(&self, _: Duration) -> Option<Duration> {
            None
        }
    }
    let (_temp, root) = root();
    let lease = Lease::acquire(&root, Duration::ZERO).unwrap();
    let handler = Arc::new(Acme::default());
    let observed = handler.clone();
    let (entered, waiting) = mpsc::channel();
    let (release, resume) = mpsc::channel();
    let server = std::thread::spawn(move || {
        server::serve(
            lease,
            identity(),
            Admission::new(),
            handler,
            &Deciding {
                entered,
                release: Mutex::new(resume),
                decisions: AtomicUsize::new(0),
            },
            server::Options::default(),
        )
    });
    waiting.recv_timeout(Duration::from_secs(5)).unwrap();
    // Each lane has its own accept thread, so arrivals are admitted while the
    // policy decides; the admission cancels that decision's exit.
    let reply = client::call(&root, &request(json!("arrival")), &options()).unwrap();
    release.send(()).unwrap();
    server.join().unwrap().unwrap();
    assert_eq!(reply.data, "arrival");
    assert_eq!(observed.dispatched.load(Ordering::SeqCst), 1);
    assert!(!root.leased().unwrap());
}

#[test]
#[cfg(unix)]
fn shutdown_refuses_arrivals_during_the_exit_decision() {
    use canonic_process_kit::core::ExitPolicy;
    use std::sync::{Mutex, mpsc};

    struct Closing {
        entered: mpsc::Sender<()>,
        release: Mutex<mpsc::Receiver<()>>,
    }
    impl ExitPolicy for Closing {
        fn exit(&self, _: Duration) -> bool {
            self.entered.send(()).unwrap();
            self.release
                .lock()
                .unwrap()
                .recv_timeout(Duration::from_secs(5))
                .unwrap();
            true
        }
        fn next_check(&self, _: Duration) -> Option<Duration> {
            None
        }
    }
    let (_temp, root) = root();
    let lease = Lease::acquire(&root, Duration::ZERO).unwrap();
    let gate = Admission::new();
    let handler = Arc::new(Acme::default());
    let observed = handler.clone();
    let (entered, waiting) = mpsc::channel();
    let (release, resume) = mpsc::channel();
    let server = std::thread::spawn(move || {
        server::serve(
            lease,
            identity(),
            gate,
            handler,
            &Closing {
                entered,
                release: Mutex::new(resume),
            },
            server::Options::default(),
        )
    });
    waiting.recv_timeout(Duration::from_secs(5)).unwrap();
    let mut ordinary = endpoint::connect(&root, false, Duration::from_secs(1)).unwrap();
    let mut control_stream = endpoint::connect(&root, true, Duration::from_secs(1)).unwrap();
    ordinary
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    control_stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    frame::write_json(
        &mut ordinary,
        &request(json!("unused")),
        frame::DEFAULT_LIMIT,
    )
    .unwrap();
    frame::write_json(
        &mut control_stream,
        &control(&root, "status"),
        frame::DEFAULT_LIMIT,
    )
    .unwrap();
    release.send(()).unwrap();
    let ordinary: canonic_process_kit::Result<Reply> =
        frame::read_json(&mut ordinary, frame::DEFAULT_LIMIT);
    let control: canonic_process_kit::Result<Reply> =
        frame::read_json(&mut control_stream, frame::DEFAULT_LIMIT);
    server.join().unwrap().unwrap();
    for reply in [ordinary, control] {
        let reply = reply.unwrap();
        assert_eq!(reply.accepted, Some(false));
        assert_eq!(reply.code, Some(Code::Draining));
    }
    assert_eq!(
        observed
            .dispatched
            .load(std::sync::atomic::Ordering::SeqCst),
        0
    );
    assert!(!root.leased().unwrap());
}

#[test]
fn saturated_work_lane_keeps_controls_responsive_and_refuses_capacity() {
    let (_temp, root) = root();
    let gate = Admission::new();
    let handler = Arc::new(Acme::default());
    *handler.blocked.lock().unwrap() = true;
    let options_server = server::Options {
        workers: 1,
        capacity: 1,
        ..Default::default()
    };
    let server = thread_server(&root, &gate, handler.clone(), options_server);
    ready(&root);
    let work_root = root.clone();
    let worker = std::thread::spawn(move || {
        client::call(&work_root, &request(json!("block")), &options()).unwrap()
    });
    wait(|| (*handler.entered.lock().unwrap() == 1).then_some(()));
    assert!(matches!(
        client::call(&root, &request(json!("excess")), &options()),
        Err(Error::Capacity)
    ));
    let status = client::control(&root, &control(&root, "status"), &options()).unwrap();
    assert_eq!(status.data["lifetime"]["work"], 1);
    let retire = client::control(&root, &control(&root, "retire_if_empty"), &options()).unwrap();
    assert_eq!(retire.data["retired"], false);
    assert_eq!(retire.data["blockers"]["items"][0], "acme-resource");
    client::control(&root, &control(&root, "drain"), &options()).unwrap();
    let refusal = client::call(&root, &request(json!("new")), &options());
    assert!(matches!(refusal, Err(Error::Draining)), "{refusal:?}");
    assert!(matches!(
        client::connect(&root, None, &options()),
        Err(Error::Draining)
    ));
    assert!(root.leased().unwrap());
    *handler.blocked.lock().unwrap() = false;
    handler.changed.notify_all();
    assert_eq!(worker.join().unwrap().data, "block");
    server.join().unwrap().unwrap();
    assert!(!root.socket(false).exists());
    assert!(!root.socket(true).exists());
    assert!(!root.leased().unwrap());
}

#[test]
fn drain_releases_lease_while_silent_control_connections_continue() {
    let (_temp, root) = root();
    let gate = Admission::new();
    let held = gate.hold().unwrap();
    let server = thread_server(
        &root,
        &gate,
        Arc::new(Acme::default()),
        server::Options {
            control_workers: 2,
            control_capacity: 8,
            io_timeout: Duration::from_millis(500),
            ..Default::default()
        },
    );
    ready(&root);
    wait(|| (gate.snapshot().observers == 0).then_some(()));
    let existing = endpoint::connect(&root, true, Duration::from_secs(1)).unwrap();
    wait(|| (gate.snapshot().observers == 1).then_some(()));
    client::control(&root, &control(&root, "drain"), &options()).unwrap();
    wait(|| (gate.snapshot().observers == 1).then_some(()));
    drop(held);
    let refusal = client::control(&root, &control(&root, "identity"), &options());
    let attempts = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let observed = attempts.clone();
    let path = root.clone();
    let (stop, stopped) = std::sync::mpsc::channel();
    let arrivals = std::thread::spawn(move || {
        let mut connections = std::collections::VecDeque::new();
        loop {
            if let Ok(stream) = endpoint::connect(&path, true, Duration::from_millis(100)) {
                connections.push_back(stream);
                if connections.len() > 4 {
                    connections.pop_front();
                }
            }
            observed.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            // Pace continuing silent arrivals while allowing immediate teardown.
            if stopped.recv_timeout(Duration::from_millis(2))
                != Err(std::sync::mpsc::RecvTimeoutError::Timeout)
            {
                break;
            }
        }
    });
    wait(|| (attempts.load(std::sync::atomic::Ordering::SeqCst) >= 8).then_some(()));
    drop(existing);
    let (finished, completion) = std::sync::mpsc::channel();
    let waiter = std::thread::spawn(move || finished.send(server.join().unwrap()).unwrap());
    let completed_during_arrivals = completion.recv_timeout(Duration::from_secs(2));
    stop.send(()).unwrap();
    arrivals.join().unwrap();
    // Even on regression, stop the workload and join the manager before asserting.
    if completed_during_arrivals.is_err() {
        completion
            .recv_timeout(Duration::from_secs(5))
            .unwrap()
            .unwrap();
    }
    waiter.join().unwrap();
    assert!(matches!(refusal, Err(Error::Draining)), "{refusal:?}");
    completed_during_arrivals.unwrap().unwrap();
    assert!(!root.socket(false).exists());
    assert!(!root.socket(true).exists());
    assert!(!root.leased().unwrap());
}

#[test]
fn controls_ignore_product_version_and_unknown_actions_have_no_effects() {
    let (_temp, root) = root();
    let gate = Admission::new();
    let handler = Arc::new(Acme::default());
    let server = thread_server(&root, &gate, handler.clone(), server::Options::default());
    ready(&root);
    let mut invalid = control(&root, "drain");
    invalid.control_version = 2;
    assert!(matches!(
        client::control(&root, &invalid, &options()),
        Err(Error::Incompatible)
    ));
    let unknown = client::control(&root, &control(&root, "future"), &options()).unwrap();
    assert_eq!(unknown.code, Some(Code::UnknownAction));
    let mut incompatible = request(Value::Null);
    incompatible.protocol += 1;
    assert!(matches!(
        client::call(&root, &incompatible, &options()),
        Err(Error::Incompatible)
    ));
    incompatible.protocol = 7;
    incompatible.requires.push("future".into());
    assert!(matches!(
        client::call(&root, &incompatible, &options()),
        Err(Error::Incompatible)
    ));
    assert_eq!(
        client::control(&root, &control(&root, "identity"), &options())
            .unwrap()
            .data["protocol"],
        7
    );
    assert_eq!(
        client::call(&root, &request(json!("ok")), &options())
            .unwrap()
            .data,
        "ok"
    );
    stop(&root, "retire_if_empty");
    server.join().unwrap().unwrap();
    assert_eq!(
        handler.dispatched.load(std::sync::atomic::Ordering::SeqCst),
        1
    );
}

#[test]
fn real_oversize_and_idle_read_deadline_release_connection_slots() {
    let (_temp, root) = root();
    let gate = Admission::new();
    let io_timeout = Duration::from_millis(200);
    let allowance = Duration::from_millis(100);
    let server = thread_server(
        &root,
        &gate,
        Arc::new(Acme::default()),
        server::Options {
            workers: 1,
            capacity: 1,
            frame_limit: 1024,
            io_timeout,
            ..Default::default()
        },
    );
    ready(&root);
    let mut stream = Stream::connect(root.socket(false)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    let mut oversized = 1025_u32.to_be_bytes().to_vec();
    oversized.extend_from_slice(&[0; 1025]);
    let oversized_started = std::time::Instant::now();
    stream.write_all(&oversized).unwrap();
    let reply: Reply = frame::read_json(&mut stream, 1024).unwrap();
    assert_eq!(reply.code, Some(Code::FrameTooLarge));
    // Keep the sender open after reading the refusal: cleanup must not wait
    // for another full frame budget or for the client to close.
    wait(|| (gate.snapshot().work == 0).then_some(()));
    let oversized_elapsed = oversized_started.elapsed();
    drop(stream);
    wait(|| {
        (client::control(&root, &control(&root, "status"), &options())
            .unwrap()
            .data["connections"]["ordinary"]
            == 0)
            .then_some(())
    });
    let silent_started = std::time::Instant::now();
    let silent = Stream::connect(root.socket(false)).unwrap();
    wait(|| (gate.snapshot().work == 1).then_some(()));
    wait(|| (gate.snapshot().work == 0).then_some(()));
    let silent_elapsed = silent_started.elapsed();
    drop(silent);
    assert_eq!(
        client::call(&root, &request(json!("ok")), &options())
            .unwrap()
            .data,
        "ok"
    );
    stop(&root, "drain");
    server.join().unwrap().unwrap();
    assert!(
        oversized_elapsed < allowance,
        "oversized frame held its slot for {oversized_elapsed:?}"
    );
    assert!(
        silent_elapsed < io_timeout + allowance,
        "silent frame held its slot for {silent_elapsed:?}"
    );
}

#[test]
fn lost_admission_ack_is_ambiguous_and_never_replayed() {
    let (_temp, root) = root();
    let lease = Lease::acquire(&root, Duration::ZERO).unwrap();
    let endpoint = lease.bind(false).unwrap();
    let path = root.clone();
    let caller =
        std::thread::spawn(move || client::call(&path, &request(json!("acme")), &options()));
    let mut stream = accept_blocking(&endpoint);
    let r: canonic_process_kit::protocol::Request =
        frame::read_json(&mut stream, frame::DEFAULT_LIMIT).unwrap();
    assert_eq!(r.request_id, "acme-request");
    drop(stream);
    assert!(
        matches!(caller.join().unwrap(), Err(Error::OutcomeUnknown(id)) if id == "acme-request")
    );
    assert!(endpoint.listener().accept().is_err());
}

/// Accept from the endpoint's nonblocking listener. Accepted sockets can keep
/// the listener's mode, so reads switch to blocking with a bounded timeout.
fn accept_blocking(endpoint: &canonic_process_kit::endpoint::Endpoint<'_>) -> Stream {
    let (stream, _) = wait(|| endpoint.listener().accept().ok());
    stream.set_nonblocking(false).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    stream
}

fn acknowledge_submission(endpoint: &canonic_process_kit::endpoint::Endpoint<'_>) -> Stream {
    let mut stream = accept_blocking(endpoint);
    let r: canonic_process_kit::protocol::Request =
        frame::read_json(&mut stream, frame::DEFAULT_LIMIT).unwrap();
    assert_eq!(r.request_id, "acme-request");
    frame::write_json(
        &mut stream,
        &json!({"control_version":1,"manager_id":"acme-manager","request_id":"acme-request","accepted":true,"code":null,"data":null}),
        frame::DEFAULT_LIMIT,
    )
    .unwrap();
    stream
}

#[test]
fn a_submitted_request_returns_its_result_after_admission() {
    let (_temp, root) = root();
    let lease = Lease::acquire(&root, Duration::ZERO).unwrap();
    let endpoint = lease.bind(false).unwrap();
    let path = root.clone();
    let caller =
        std::thread::spawn(move || client::submit(&path, &request(json!("acme")), &options()));
    let mut stream = acknowledge_submission(&endpoint);
    let admitted = caller.join().unwrap().unwrap();
    assert_eq!(admitted.acknowledgement().manager_id, "acme-manager");
    frame::write_json(
        &mut stream,
        &json!({"control_version":1,"manager_id":"acme-manager","request_id":"acme-request","accepted":null,"code":null,"data":"done"}),
        frame::DEFAULT_LIMIT,
    )
    .unwrap();
    assert_eq!(admitted.result().unwrap().data, json!("done"));
}

#[test]
fn an_abandoned_submission_closes_its_connection_without_replay() {
    let (_temp, root) = root();
    let lease = Lease::acquire(&root, Duration::ZERO).unwrap();
    let endpoint = lease.bind(false).unwrap();
    let path = root.clone();
    let caller =
        std::thread::spawn(move || client::submit(&path, &request(json!("acme")), &options()));
    let mut stream = acknowledge_submission(&endpoint);
    let admitted = caller.join().unwrap().unwrap();
    let abandon = admitted.abandon_handle().unwrap();
    let waiting = std::thread::spawn(move || admitted.result());
    abandon.abandon();
    assert!(
        matches!(waiting.join().unwrap(), Err(Error::OutcomeUnknown(id)) if id == "acme-request")
    );
    // The manager side observes the client's departure.
    stream
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    let mut byte = [0_u8; 1];
    assert_eq!(std::io::Read::read(&mut stream, &mut byte).unwrap(), 0);
    assert!(endpoint.listener().accept().is_err());
}

#[test]
fn client_admission_frame_deadline_does_not_renew_on_partial_reads() {
    assert_trickled_reply_times_out(true);
}

#[test]
fn client_result_frame_deadline_does_not_renew_on_partial_reads() {
    assert_trickled_reply_times_out(false);
}

#[test]
fn readiness_deadline_bounds_a_silent_endpoint() {
    assert_readiness_deadline(false);
}

#[test]
fn readiness_deadline_spans_admission_and_identity_frames() {
    assert_readiness_deadline(true);
}

fn assert_readiness_deadline(send_admission: bool) {
    let (_temp, root) = root();
    let lease = Lease::acquire(&root, Duration::ZERO).unwrap();
    let endpoint = lease.bind(true).unwrap();
    let path = root.clone();
    let (finished, result) = std::sync::mpsc::channel();
    let caller = std::thread::spawn(move || {
        let options = client::Options {
            readiness: Duration::from_millis(100),
            io: Duration::from_secs(3),
            ..options()
        };
        finished
            .send(client::connect(&path, None, &options))
            .unwrap();
    });
    let (mut stream, _) = wait(|| endpoint.listener().accept().ok());
    stream.set_nonblocking(false).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    stream
        .set_write_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let request: canonic_process_kit::protocol::Control =
        frame::read_json(&mut stream, frame::DEFAULT_LIMIT).unwrap();
    assert_eq!(request.action, "identity");
    let mut outcome = None;
    if send_admission {
        let admission = Reply {
            control_version: 1,
            manager_id: identity().manager_id,
            request_id: request.request_id,
            accepted: Some(true),
            code: None,
            data: Value::Null,
        };
        let reply = Reply {
            accepted: None,
            data: json!(identity()),
            ..admission.clone()
        };
        for frame in [&admission, &reply] {
            // Both frames individually fit io and even readiness, but their
            // combined delay exceeds readiness. Neither may renew that budget.
            match result.recv_timeout(Duration::from_millis(75)) {
                Ok(value) => {
                    outcome = Some(value);
                    break;
                }
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                    let _ = frame::write_json(&mut stream, frame, frame::DEFAULT_LIMIT);
                }
                Err(error) => panic!("fixture client stopped: {error}"),
            }
        }
    }
    // Keep the endpoint open while waiting. Closing it first would hide an
    // overlong probe by waking the client with EOF instead of its own deadline.
    let outcome = outcome.map_or_else(|| result.recv_timeout(Duration::from_secs(1)), Ok);
    drop(stream);
    caller.join().unwrap();
    assert!(matches!(outcome, Ok(Err(Error::Starting))), "{outcome:?}");
    assert!(
        endpoint.listener().accept().is_err(),
        "probe continued after its deadline"
    );
}

fn assert_trickled_reply_times_out(trickle_admission: bool) {
    let (_temp, root) = root();
    let lease = Lease::acquire(&root, Duration::ZERO).unwrap();
    let endpoint = lease.bind(false).unwrap();
    let path = root.clone();
    let (finished, result) = std::sync::mpsc::channel();
    let caller = std::thread::spawn(move || {
        let options = client::Options {
            io: Duration::from_millis(200),
            ..options()
        };
        finished
            .send(client::call(&path, &request(json!("acme")), &options))
            .unwrap();
    });
    let (mut stream, _) = wait(|| endpoint.listener().accept().ok());
    stream.set_nonblocking(false).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    stream
        .set_write_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let request: canonic_process_kit::protocol::Request =
        frame::read_json(&mut stream, frame::DEFAULT_LIMIT).unwrap();
    let admission = Reply {
        control_version: 1,
        manager_id: "acme-manager".into(),
        request_id: request.request_id.clone(),
        accepted: Some(true),
        code: None,
        data: Value::Null,
    };
    let reply = Reply {
        accepted: None,
        data: json!("acme-result"),
        ..admission.clone()
    };
    if !trickle_admission {
        frame::write_json(&mut stream, &admission, frame::DEFAULT_LIMIT).unwrap();
    }
    let mut bytes = vec![];
    frame::write_json(
        &mut bytes,
        if trickle_admission {
            &admission
        } else {
            &reply
        },
        frame::DEFAULT_LIMIT,
    )
    .unwrap();
    let middle = 4 + (bytes.len() - 4) / 2;
    stream.write_all(&bytes[..2]).unwrap();
    let mut outcome = None;
    for chunk in [&bytes[2..middle], &bytes[middle..]] {
        // Each gap is shorter than io, but the complete prefix and body take
        // longer than io. Stop pacing immediately when the client finishes.
        match result.recv_timeout(Duration::from_millis(150)) {
            Ok(value) => {
                outcome = Some(value);
                break;
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                // Closing on deadline can race the next fixture write.
                let _ = stream.write_all(chunk);
            }
            Err(error) => panic!("fixture client stopped: {error}"),
        }
    }
    if trickle_admission && outcome.is_none() {
        let _ = frame::write_json(&mut stream, &reply, frame::DEFAULT_LIMIT);
    }
    let outcome = outcome.unwrap_or_else(|| result.recv_timeout(Duration::from_secs(5)).unwrap());
    drop(stream);
    caller.join().unwrap();
    assert!(matches!(outcome, Err(Error::OutcomeUnknown(id)) if id == request.request_id));
    assert!(
        endpoint.listener().accept().is_err(),
        "timed-out work was replayed"
    );
}
