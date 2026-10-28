//! Upgrade reuse, incarnation checks and admission-independent control acceptance.

use super::fixtures::*;
use canonic_manager::{
    Error,
    client::{self, Upgrade},
    core::Admission,
    endpoint::Root,
    endpoint::{Endpoint, Lease},
    frame,
    launch::Launcher,
    protocol::{Code, Control, Reply},
    server,
};
use serde_json::json;
use std::sync::Arc;
use std::{os::unix::net::UnixStream, time::Duration};

fn read_control(endpoint: &Endpoint<'_>) -> (UnixStream, Control) {
    let (mut stream, _) = wait(|| endpoint.listener().accept().ok());
    stream.set_nonblocking(false).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    stream
        .set_write_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let request = frame::read_json(&mut stream, frame::DEFAULT_LIMIT).unwrap();
    (stream, request)
}

fn reply_control(
    stream: &mut UnixStream,
    request: &Control,
    data: serde_json::Value,
    result: bool,
) {
    let ack = Reply {
        control_version: 1,
        manager_id: identity().manager_id,
        request_id: request.request_id.clone(),
        accepted: Some(true),
        code: None,
        data: serde_json::Value::Null,
    };
    frame::write_json(stream, &ack, frame::DEFAULT_LIMIT).unwrap();
    if result {
        let reply = Reply {
            accepted: None,
            data,
            ..ack
        };
        frame::write_json(stream, &reply, frame::DEFAULT_LIMIT).unwrap();
    }
}

struct FixtureLaunch {
    root: Root,
    mode: &'static str,
    children: std::sync::Mutex<Vec<ChildGuard>>,
}
impl Launcher for FixtureLaunch {
    fn start(&self) -> canonic_manager::Result<()> {
        let child = child(&self.root, self.mode);
        self.children.lock().unwrap().push(child);
        Ok(())
    }
}
impl FixtureLaunch {
    fn finish(&self) {
        wait(|| {
            let mut children = self.children.lock().unwrap();
            let running = children
                .iter_mut()
                .map(|child| child.0.try_wait().unwrap().is_none())
                .filter(|running| *running)
                .count();
            (running == 1).then_some(())
        });
        stop(&self.root, "drain");
        let children = std::mem::take(&mut *self.children.lock().unwrap());
        for mut child in children {
            let status = child.0.wait().unwrap();
            assert!(status.success() || status.code() == Some(73));
        }
    }
}

#[test]
fn concurrent_versioned_clients_converge_on_one_replacement() {
    let (_temp, root) = root();
    let mut older = child(&root, "manager");
    let old_id = ready(&root).manager_id;
    let launch = Arc::new(FixtureLaunch {
        root: root.clone(),
        mode: "manager-v2",
        children: std::sync::Mutex::new(vec![]),
    });
    let barrier = Arc::new(std::sync::Barrier::new(8));
    let clients: Vec<_> = (0..8)
        .map(|_| {
            let root = root.clone();
            let launch = launch.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                client::connect_versioned(&root, launch.as_ref(), "2.0.0", &options())
            })
        })
        .collect();
    let results: Vec<_> = clients
        .into_iter()
        .map(|client| client.join().unwrap())
        .collect();
    let identities: Vec<_> = results.into_iter().map(Result::unwrap).collect();
    assert!(identities.iter().all(|id| id.build == "2.0.0"
        && id.manager_id == identities[0].manager_id
        && id.manager_id != old_id));
    assert!(wait(|| older.0.try_wait().unwrap()).success());
    launch.finish();
}

#[test]
fn versioned_connection_replaces_an_empty_older_process() {
    let (_temp, root) = root();
    let mut older = child(&root, "manager");
    let old_id = ready(&root).manager_id;
    let launch = FixtureLaunch {
        root: root.clone(),
        mode: "manager-v2",
        children: std::sync::Mutex::new(vec![]),
    };
    let current = client::connect_versioned(&root, &launch, "2.0.0", &options()).unwrap();
    assert_eq!(current.build, "2.0.0");
    assert_ne!(current.manager_id, old_id);
    assert!(wait(|| older.0.try_wait().unwrap()).success());
    assert_eq!(
        client::call(&root, &request(json!("new-version")), &options())
            .unwrap()
            .data,
        "new-version"
    );
    launch.finish();
}

#[test]
fn versioned_connection_refuses_a_stale_launcher_without_restart_churn() {
    let (_temp, root) = root();
    let mut older = child(&root, "manager");
    let old_id = ready(&root).manager_id;
    let launch = FixtureLaunch {
        root: root.clone(),
        mode: "manager",
        children: std::sync::Mutex::new(vec![]),
    };
    assert!(matches!(
        client::connect_versioned(&root, &launch, "2.0.0", &options()),
        Err(Error::Incompatible)
    ));
    assert!(wait(|| older.0.try_wait().unwrap()).success());
    let replacement = ready(&root);
    assert_ne!(replacement.manager_id, old_id);
    assert_eq!(replacement.build, "1.0.0");
    let status = client::control(&root, &control(&root, "status"), &options()).unwrap();
    assert_eq!(status.data["lifetime"]["retirement_pending"], false);
    launch.finish();
}

#[test]
fn newer_client_schedules_busy_retirement_without_interrupting_work() {
    let (_temp, root) = root();
    let gate = Admission::new();
    let held = gate.hold().unwrap();
    let server = thread_server(
        &root,
        &gate,
        Arc::new(Acme::default()),
        server::Options::default(),
    );
    let original = ready(&root);
    let launch = FixtureLaunch {
        root: root.clone(),
        mode: "manager-v2",
        children: std::sync::Mutex::new(vec![]),
    };
    let current = client::connect_versioned(&root, &launch, "2.0.0", &options()).unwrap();
    assert_eq!(current.manager_id, original.manager_id);
    assert!(gate.snapshot().retirement_pending);
    assert_eq!(
        client::call(&root, &request(json!("still-supervised")), &options())
            .unwrap()
            .data,
        "still-supervised"
    );
    assert_eq!(launch.children.lock().unwrap().len(), 0);
    drop(held);
    wait(|| server.is_finished().then_some(()));
    server.join().unwrap().unwrap();
    assert!(!root.leased().unwrap());
}

#[test]
fn equal_older_and_metadata_only_versions_do_not_retire_a_manager() {
    let (_temp, root) = root();
    let gate = Admission::new();
    let server = thread_server(
        &root,
        &gate,
        Arc::new(Acme::default()),
        server::Options::default(),
    );
    let original = ready(&root);
    let launch = FixtureLaunch {
        root: root.clone(),
        mode: "manager-v2",
        children: std::sync::Mutex::new(vec![]),
    };
    for version in ["0.9.0", "1.0.0", "1.0.0+acme-rebuild", "1.0.0-rc.1"] {
        let current = client::connect_versioned(&root, &launch, version, &options()).unwrap();
        assert_eq!(current.manager_id, original.manager_id);
        assert!(!gate.snapshot().retirement_pending);
    }
    assert_eq!(launch.children.lock().unwrap().len(), 0);
    stop(&root, "drain");
    server.join().unwrap().unwrap();
}

#[test]
fn manager_validates_version_and_incarnation_before_scheduling_retirement() {
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
    let mut r = control(&root, "retire_if_older");
    for version in ["0.9.0", "1.0.0", "1.0.0+acme.2"] {
        r.arguments = json!({"version":version});
        assert_eq!(
            client::control(&root, &r, &options()).unwrap().data["newer"],
            false
        );
        assert!(!gate.snapshot().retirement_pending);
    }
    r.arguments = json!({"version":"acme-dev"});
    assert!(matches!(
        client::control(&root, &r, &options()),
        Err(Error::InvalidVersion)
    ));
    assert!(!gate.snapshot().retirement_pending);
    r.arguments = json!({"version":"2.0.0"});
    r.manager_id = Some("acme-stale".into());
    assert_eq!(
        client::control(&root, &r, &options()).unwrap().code,
        Some(Code::StaleManager)
    );
    assert!(!gate.snapshot().retirement_pending);
    r.manager_id = Some(identity().manager_id);
    let scheduled = client::control(&root, &r, &options()).unwrap();
    assert_eq!(scheduled.data["retired"], false);
    assert!(gate.snapshot().retirement_pending);
    drop(held);
    wait(|| server.is_finished().then_some(()));
    server.join().unwrap().unwrap();
}

#[test]
fn invalid_client_release_starts_nothing() {
    let (_temp, root) = root();
    let launch = FixtureLaunch {
        root: root.clone(),
        mode: "manager-v2",
        children: std::sync::Mutex::new(vec![]),
    };
    assert!(matches!(
        client::connect_versioned(&root, &launch, "acme-dev", &options()),
        Err(Error::InvalidVersion)
    ));
    assert_eq!(launch.children.lock().unwrap().len(), 0);
    assert!(!root.path().exists());
}

#[test]
fn pre_feature_busy_manager_uses_the_existing_empty_retirement_control() {
    let (_temp, root) = root();
    let lease = Lease::acquire(&root, Duration::ZERO).unwrap();
    let endpoint = lease.bind(true).unwrap();
    let launch = Arc::new(FixtureLaunch {
        root: root.clone(),
        mode: "manager-v2",
        children: std::sync::Mutex::new(vec![]),
    });
    let client_root = root.clone();
    let client_launch = launch.clone();
    let caller = std::thread::spawn(move || {
        client::connect_versioned(&client_root, client_launch.as_ref(), "2.0.0", &options())
    });
    let (mut stream, identity_request) = read_control(&endpoint);
    assert_eq!(identity_request.action, "identity");
    reply_control(&mut stream, &identity_request, json!(identity()), true);
    drop(stream);
    let (mut stream, retirement) = read_control(&endpoint);
    assert_eq!(retirement.action, "retire_if_empty");
    assert_eq!(retirement.manager_id, Some(identity().manager_id));
    assert!(retirement.arguments.is_null());
    reply_control(
        &mut stream,
        &retirement,
        json!({"retired":false,"blockers":["acme-resource"]}),
        true,
    );
    drop(stream);
    let result = caller.join().unwrap().unwrap();
    assert_eq!(result.build, "1.0.0");
    assert_eq!(launch.children.lock().unwrap().len(), 0);
}

#[test]
fn lost_version_retirement_result_reobserves_before_starting_a_replacement() {
    let (_temp, root) = root();
    let lease = Lease::acquire(&root, Duration::ZERO).unwrap();
    let endpoint = lease.bind(true).unwrap();
    let launch = Arc::new(FixtureLaunch {
        root: root.clone(),
        mode: "manager-v2",
        children: std::sync::Mutex::new(vec![]),
    });
    let client_root = root.clone();
    let client_launch = launch.clone();
    let caller = std::thread::spawn(move || {
        client::connect_versioned(&client_root, client_launch.as_ref(), "2.0.0", &options())
    });
    let (mut stream, identity_request) = read_control(&endpoint);
    let mut old = identity();
    old.capabilities
        .push(canonic_manager::protocol::VERSION_RETIREMENT.into());
    reply_control(&mut stream, &identity_request, json!(old), true);
    drop(stream);
    let (mut stream, retirement) = read_control(&endpoint);
    assert_eq!(retirement.action, "retire_if_older");
    reply_control(&mut stream, &retirement, serde_json::Value::Null, false);
    // Model manager death after admission: the result is lost and the kernel
    // lease disappears before the client's next identity observation.
    drop(endpoint);
    drop(lease);
    drop(stream);
    let current = caller.join().unwrap().unwrap();
    assert_eq!(current.build, "2.0.0");
    assert_ne!(current.manager_id, identity().manager_id);
    launch.finish();
}

#[test]
fn an_unorderable_running_build_is_not_retired() {
    let (_temp, root) = root();
    let lease = Lease::acquire(&root, Duration::ZERO).unwrap();
    let gate = Admission::new();
    let server_gate = gate.clone();
    let mut unknown = identity();
    unknown.build = "acme-development".into();
    let server = std::thread::spawn(move || {
        server::serve(
            lease,
            unknown,
            server_gate,
            Arc::new(Acme::default()),
            &canonic_manager::core::Explicit,
            server::Options::default(),
        )
    });
    let running = ready(&root);
    assert!(
        !running
            .capabilities
            .iter()
            .any(|c| c == canonic_manager::protocol::VERSION_RETIREMENT)
    );
    let launch = FixtureLaunch {
        root: root.clone(),
        mode: "manager-v2",
        children: std::sync::Mutex::new(vec![]),
    };
    assert!(matches!(
        client::connect_versioned(&root, &launch, "2.0.0", &options()),
        Err(Error::InvalidVersion)
    ));
    assert!(!gate.snapshot().retirement_pending);
    assert_eq!(launch.children.lock().unwrap().len(), 0);
    stop(&root, "drain");
    server.join().unwrap().unwrap();
}

#[test]
fn newer_client_retires_empty_older_manager_and_reuses_busy_compatible_one() {
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
    let upgrade = client::upgrade(&root, 7, &["acme_v1".into()], &options()).unwrap();
    assert!(
        matches!(upgrade, Upgrade::Busy { compatible: true, blockers, .. } if blockers["items"][0] == "acme-resource")
    );
    assert_eq!(
        client::call(&root, &request(json!("still-usable")), &options())
            .unwrap()
            .data,
        "still-usable"
    );
    let incompatible = client::upgrade(&root, 8, &[], &options()).unwrap();
    assert!(matches!(
        incompatible,
        Upgrade::Busy {
            compatible: false,
            ..
        }
    ));
    assert!(client::control(&root, &control(&root, "identity"), &options()).is_ok());
    drop(held);
    wait(|| {
        matches!(
            client::upgrade(&root, 8, &[], &options()).unwrap(),
            Upgrade::Retired
        )
        .then_some(())
    });
    server.join().unwrap().unwrap();
}

#[test]
fn stale_incarnation_cannot_retire_or_drain_replacement() {
    let (_temp, root) = root();
    let gate = Admission::new();
    let server = thread_server(
        &root,
        &gate,
        Arc::new(Acme::default()),
        server::Options::default(),
    );
    ready(&root);
    for action in ["drain", "retire_if_empty"] {
        let mut r = control(&root, action);
        r.manager_id = Some("acme-old-incarnation".into());
        assert_eq!(
            client::control(&root, &r, &options()).unwrap().code,
            Some(Code::StaleManager)
        );
    }
    assert_eq!(
        client::call(&root, &request(json!("ok")), &options())
            .unwrap()
            .data,
        "ok"
    );
    stop(&root, "drain");
    server.join().unwrap().unwrap();
}
