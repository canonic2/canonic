//! Real startup races, lease death, parent signals and minimal-environment acceptance.

use super::fixtures::*;
use canonic_background_runtime::{Error, client, endpoint::Lease, launch::Launcher};
use serde_json::json;
use std::{
    io::Write,
    sync::{Arc, Barrier},
    time::Duration,
};

#[test]
fn one_client_activates_once_while_its_child_has_not_acquired_the_lease() {
    struct Delayed {
        root: canonic_background_runtime::endpoint::Root,
        children: std::sync::Mutex<Vec<ChildGuard>>,
    }
    impl Launcher for Delayed {
        fn start(&self) -> canonic_background_runtime::Result<()> {
            self.children
                .lock()
                .unwrap()
                .push(child(&self.root, "delayed-manager"));
            Ok(())
        }
    }
    let (_temp, root) = root();
    let launch = Delayed {
        root: root.clone(),
        children: std::sync::Mutex::new(vec![]),
    };
    let mut options = options();
    options.readiness = Duration::from_millis(100);
    let result = client::connect(&root, Some(&launch), &options);
    assert!(matches!(result, Err(Error::Starting)));
    assert!(!root.leased().unwrap());
    let mut children = std::mem::take(&mut *launch.children.lock().unwrap());
    let activations = children.len();
    let mut winner = children.remove(0);
    // Clean up every delayed fixture even when the activation count is wrong.
    drop(children);
    winner.0.stdin.as_mut().unwrap().write_all(b"x").unwrap();
    ready(&root);
    stop(&root, "drain");
    assert!(winner.0.wait().unwrap().success());
    assert_eq!(activations, 1);
}

#[test]
fn simultaneous_starters_converge_and_losers_exit_distinctly() {
    let (_temp, root) = root();
    let starters: Vec<_> = (0..6)
        .map(|_| spawn(&root).start_reaped().unwrap())
        .collect();
    let winner = ready(&root);
    let clients: Vec<_> = (0..12)
        .map(|_| {
            let root = root.clone();
            std::thread::spawn(move || client::connect(&root, None, &options()).unwrap().manager_id)
        })
        .collect();
    for client in clients {
        assert_eq!(client.join().unwrap(), winner.manager_id);
    }
    wait(|| (starters.iter().filter(|s| s.is_finished()).count() == 5).then_some(()));
    stop(&root, "drain");
    let outcomes: Vec<_> = starters
        .into_iter()
        .map(|child| child.join().unwrap().unwrap())
        .collect();
    assert_eq!(outcomes.iter().filter(|s| s.success()).count(), 1);
    assert!(
        outcomes
            .iter()
            .filter(|s| !s.success())
            .all(|s| s.code() == Some(73))
    );
}

#[test]
fn clients_connect_or_start_race_reaches_one_winner() {
    struct Tracked {
        launch: canonic_background_runtime::launch::Spawn,
        reapers: std::sync::Mutex<
            Vec<std::thread::JoinHandle<std::io::Result<std::process::ExitStatus>>>,
        >,
    }
    impl Launcher for Tracked {
        fn start(&self) -> canonic_background_runtime::Result<()> {
            self.reapers
                .lock()
                .unwrap()
                .push(self.launch.start_reaped()?);
            Ok(())
        }
    }
    let (_temp, root) = root();
    let barrier = Arc::new(Barrier::new(8));
    let launch = Arc::new(Tracked {
        launch: spawn(&root),
        reapers: std::sync::Mutex::new(vec![]),
    });
    let clients: Vec<_> = (0..8)
        .map(|_| {
            let root = root.clone();
            let barrier = barrier.clone();
            let launch = launch.clone();
            std::thread::spawn(move || {
                barrier.wait();
                client::connect(&root, Some(launch.as_ref()), &options())
                    .unwrap()
                    .manager_id
            })
        })
        .collect();
    let ids: Vec<_> = clients.into_iter().map(|c| c.join().unwrap()).collect();
    assert!(ids.iter().all(|id| id == &ids[0]));
    wait(|| {
        let reapers = launch.reapers.lock().unwrap();
        (reapers.iter().filter(|h| !h.is_finished()).count() == 1).then_some(())
    });
    stop(&root, "drain");
    wait(|| (!root.leased().unwrap()).then_some(()));
    for reaper in launch.reapers.lock().unwrap().drain(..) {
        let status = reaper.join().unwrap().unwrap();
        assert!(status.success() || status.code() == Some(73));
    }
}

#[test]
fn starting_owner_waits_for_exiting_lease_holder() {
    let (_temp, root) = root();
    let lease = Lease::acquire(&root, Duration::ZERO).unwrap();
    let mut child = child(&root, "waiting");
    wait(|| root.path().join("acme-waiting").exists().then_some(()));
    assert!(child.0.try_wait().unwrap().is_none());
    assert!(matches!(
        client::connect(&root, None, &options()),
        Err(Error::Starting)
    ));
    drop(lease);
    ready(&root);
    stop(&root, "drain");
    assert!(child.0.wait().unwrap().success());
}

#[test]
fn sigkill_releases_kernel_lease_and_next_client_replaces_stale_socket() {
    let (_temp, root) = root();
    let mut first = child(&root, "manager");
    let original = ready(&root);
    first.0.kill().unwrap();
    first.0.wait().unwrap();
    assert!(!root.leased().unwrap());
    assert!(root.socket(false).exists());
    let launch = spawn(&root);
    let reap = launch.start_reaped().unwrap();
    assert_ne!(ready(&root).manager_id, original.manager_id);
    stop(&root, "drain");
    assert!(reap.join().unwrap().unwrap().success());
}

#[test]
fn detached_start_has_only_allowlist_null_stdin_and_new_session() {
    let (_temp, root) = root();
    let reap = spawn(&root).start_reaped().unwrap();
    ready(&root);
    let reply = client::call(&root, &request(json!("environment")), &options()).unwrap();
    let data = reply.data;
    assert_eq!(data["env"].as_object().unwrap().len(), 2);
    assert_eq!(data["env"]["ACME_MODE"], "manager");
    assert_eq!(data["stdin"], 0);
    assert_eq!(data["pid"], data["sid"]);
    assert_eq!(data["pid"], data["group"]);
    stop(&root, "drain");
    assert!(reap.join().unwrap().unwrap().success());
}

#[test]
fn detached_manager_survives_parent_exit_and_parent_group_signals() {
    for signal in [
        None,
        Some(nix::sys::signal::Signal::SIGTERM),
        Some(nix::sys::signal::Signal::SIGHUP),
    ] {
        let (_temp, root) = root();
        let mut parent = child(&root, "parent");
        ready(&root);
        if let Some(signal) = signal {
            nix::sys::signal::killpg(
                nix::unistd::Pid::from_raw(i32::try_from(parent.0.id()).unwrap()),
                signal,
            )
            .unwrap();
        } else {
            parent.0.stdin.take();
        }
        parent.0.wait().unwrap();
        assert_eq!(
            client::call(&root, &request(json!("survived")), &options())
                .unwrap()
                .data,
            "survived"
        );
        stop(&root, "drain");
        wait(|| (!root.leased().unwrap()).then_some(()));
    }
}

#[test]
fn account_home_ignores_home_environment_in_a_separate_process() {
    let (_temp, root) = root();
    let mut command = std::process::Command::new(std::env::current_exe().unwrap());
    let status = command
        .args(["--exact", "fixtures::entry", "--nocapture"])
        .env("ACME_ROOT", root.path())
        .env("ACME_MODE", "home")
        .env("HOME", "/tmp/acme-not-account-home")
        .status()
        .unwrap();
    assert!(status.success());
}

#[test]
fn service_launch_path_is_selected_without_spawning_the_binary() {
    struct Registered {
        root: canonic_background_runtime::endpoint::Root,
        gate: canonic_background_runtime::core::Admission,
        count: std::sync::atomic::AtomicUsize,
        running: std::sync::Mutex<
            Option<std::thread::JoinHandle<canonic_background_runtime::Result<()>>>,
        >,
    }
    impl Launcher for Registered {
        fn start(&self) -> canonic_background_runtime::Result<()> {
            self.count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            *self.running.lock().unwrap() = Some(thread_server(
                &self.root,
                &self.gate,
                Arc::new(Acme::default()),
                canonic_background_runtime::server::Options::default(),
            ));
            Ok(())
        }
    }
    let (_temp, root) = root();
    let registered = Registered {
        root: root.clone(),
        gate: canonic_background_runtime::core::Admission::new(),
        count: std::sync::atomic::AtomicUsize::new(0),
        running: std::sync::Mutex::new(None),
    };
    let id = client::connect(&root, Some(&registered), &options()).unwrap();
    assert_eq!(id.manager_id, identity().manager_id);
    assert_eq!(
        registered.count.load(std::sync::atomic::Ordering::SeqCst),
        1
    );
    stop(&root, "drain");
    registered
        .running
        .lock()
        .unwrap()
        .take()
        .unwrap()
        .join()
        .unwrap()
        .unwrap();
}
