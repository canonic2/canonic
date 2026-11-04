//! Real startup races, lease death, parent signals and minimal-environment acceptance.

use super::fixtures::*;
use canonic_process_kit::{
    Error, client,
    core::{Admission, Explicit},
    endpoint::Lease,
    launch::{Launched, Launcher},
    server,
};
use serde_json::json;
use std::{
    io::Write,
    sync::{Arc, Barrier},
    time::Duration,
};

#[test]
fn one_client_activates_once_while_its_child_has_not_acquired_the_lease() {
    struct Delayed {
        root: canonic_process_kit::endpoint::Root,
        children: std::sync::Mutex<Vec<ChildGuard>>,
    }
    impl Launcher for Delayed {
        fn start(&self) -> canonic_process_kit::Result<()> {
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
        launch: canonic_process_kit::launch::Spawn,
        reapers: std::sync::Mutex<
            Vec<std::thread::JoinHandle<std::io::Result<std::process::ExitStatus>>>,
        >,
    }
    impl Launcher for Tracked {
        fn start(&self) -> canonic_process_kit::Result<()> {
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
fn a_launched_manager_that_fails_before_readiness_is_reported_with_its_status() {
    let (_temp, root) = root();
    let mut launch = spawn(&root);
    launch.environment[1].1 = "fails-at-startup".into();
    // A timeout would return `Starting`; `Exited` means the wait ended on the exit.
    match client::connect(&root, Some(&launch), &options()) {
        Err(Error::Exited(status)) => assert_eq!(status.code(), Some(70)),
        other => panic!("expected the launched manager's exit, got {other:?}"),
    }
    assert!(!root.leased().unwrap());
}

#[test]
fn a_launched_child_that_fails_after_losing_the_lease_does_not_end_the_wait() {
    type Server = std::thread::JoinHandle<canonic_process_kit::Result<()>>;
    /// Takes the lease, then starts a child that loses it and exits 73; the
    /// lease holder serves only once that child has exited.
    struct Lost {
        root: canonic_process_kit::endpoint::Root,
        server: std::sync::Mutex<Option<Server>>,
    }
    impl Launcher for Lost {
        fn start(&self) -> canonic_process_kit::Result<()> {
            unreachable!("connect launches through launch")
        }
        fn launch(&self) -> canonic_process_kit::Result<Launched> {
            let lease = Lease::acquire(&self.root, Duration::ZERO)?;
            let mut loser = child(&self.root, "manager");
            let (exited, observed) = std::sync::mpsc::channel();
            let reaper = std::thread::spawn(move || {
                let status = loser.0.wait();
                let _ = exited.send(());
                status
            });
            *self.server.lock().unwrap() = Some(std::thread::spawn(move || {
                let _ = observed.recv();
                server::serve(
                    lease,
                    identity(),
                    Admission::new(),
                    Arc::new(Acme::default()),
                    &Explicit,
                    server::Options::default(),
                )
            }));
            Ok(Launched::reaped(reaper))
        }
    }
    let (_temp, root) = root();
    root.prepare().unwrap();
    let launch = Lost {
        root: root.clone(),
        server: std::sync::Mutex::new(None),
    };
    let reached = client::connect(&root, Some(&launch), &options());
    stop(&root, "drain");
    let server = launch.server.lock().unwrap().take().unwrap();
    server.join().unwrap().unwrap();
    assert_eq!(reached.unwrap().manager_id, identity().manager_id);
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
    let mut names: Vec<_> = data["env"].as_object().unwrap().keys().cloned().collect();
    names.sort_by_key(|name| name.to_ascii_uppercase());
    // Windows adds only SystemRoot, which sockets need.
    #[cfg(unix)]
    assert_eq!(names, ["ACME_MODE", "ACME_ROOT"]);
    #[cfg(windows)]
    assert_eq!(names, ["ACME_MODE", "ACME_ROOT", "SystemRoot"]);
    assert_eq!(data["env"]["ACME_MODE"], "manager");
    assert_eq!(data["stdin"], 0);
    #[cfg(unix)]
    {
        assert_eq!(data["pid"], data["sid"]);
        assert_eq!(data["pid"], data["group"]);
    }
    stop(&root, "drain");
    assert!(reap.join().unwrap().unwrap().success());
}

#[test]
#[cfg(windows)]
fn detached_manager_survives_parent_exit_termination_console_break_and_job_close() {
    for ending in ["exit", "terminate", "break", "job"] {
        let (_temp, root) = root();
        let mut parent = child(&root, "parent-on-input");
        // The job must hold the parent before it starts the manager.
        let job = (ending == "job").then(|| windows_job::kill_on_close(&parent.0));
        parent.0.stdin.as_mut().unwrap().write_all(b"x").unwrap();
        ready(&root);
        match ending {
            "exit" => drop(parent.0.stdin.take()),
            "terminate" => parent.0.kill().unwrap(),
            "break" => windows_job::console_break(&parent.0),
            _ => drop(job),
        }
        parent.0.wait().unwrap();
        assert_eq!(
            client::call(&root, &request(json!("survived")), &options())
                .unwrap()
                .data,
            "survived",
            "{ending}"
        );
        stop(&root, "drain");
        wait(|| (!root.leased().unwrap()).then_some(()));
    }
}

#[cfg(windows)]
#[allow(
    unsafe_code,
    reason = "std exposes no job-object or console-event calls; each block states its invariants"
)]
mod windows_job {
    use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
    use windows_sys::Win32::System::{
        Console::{CTRL_BREAK_EVENT, GenerateConsoleCtrlEvent},
        JobObjects::{
            AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_BREAKAWAY_OK,
            JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
            JobObjectExtendedLimitInformation, SetInformationJobObject,
        },
    };

    /// A job that ends its processes when closed, like a terminal or remote
    /// session's job, and lets children break away.
    pub(super) fn kill_on_close(process: &std::process::Child) -> OwnedHandle {
        // SAFETY: null attributes and name create an unnamed job with the
        // default descriptor; a non-null result is a handle this call owns.
        let raw = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
        assert!(!raw.is_null(), "{}", std::io::Error::last_os_error());
        // SAFETY: `raw` is an open handle nothing else owns.
        let job = unsafe { OwnedHandle::from_raw_handle(raw) };
        let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        limits.BasicLimitInformation.LimitFlags =
            JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE | JOB_OBJECT_LIMIT_BREAKAWAY_OK;
        // SAFETY: the pointer and size describe `limits`, which outlives the call.
        let set = unsafe {
            SetInformationJobObject(
                job.as_raw_handle(),
                JobObjectExtendedLimitInformation,
                (&raw const limits).cast(),
                u32::try_from(size_of_val(&limits)).unwrap(),
            )
        };
        assert_ne!(set, 0, "{}", std::io::Error::last_os_error());
        // SAFETY: both handles are open for the duration of the call.
        let assigned =
            unsafe { AssignProcessToJobObject(job.as_raw_handle(), process.as_raw_handle()) };
        assert_ne!(assigned, 0, "{}", std::io::Error::last_os_error());
        job
    }

    /// Ctrl+Break to the process group the fixture child leads, on the console
    /// this test shares with it.
    pub(super) fn console_break(process: &std::process::Child) {
        // SAFETY: the call takes only plain integers.
        let sent = unsafe { GenerateConsoleCtrlEvent(CTRL_BREAK_EVENT, process.id()) };
        assert_ne!(sent, 0, "{}", std::io::Error::last_os_error());
    }
}

#[test]
#[cfg(unix)]
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
    command
        .args(["--exact", "fixtures::entry", "--nocapture"])
        .env("ACME_ROOT", root.path())
        .env("ACME_MODE", "home");
    let changed = root.path().join("acme-not-account-home");
    #[cfg(windows)]
    command
        .env("ACME_PROFILE", std::env::var_os("USERPROFILE").unwrap())
        .env("USERPROFILE", &changed);
    let status = command.env("HOME", &changed).status().unwrap();
    assert!(status.success());
}

#[test]
fn service_launch_path_is_selected_without_spawning_the_binary() {
    struct Registered {
        root: canonic_process_kit::endpoint::Root,
        gate: canonic_process_kit::core::Admission,
        count: std::sync::atomic::AtomicUsize,
        running: std::sync::Mutex<Option<std::thread::JoinHandle<canonic_process_kit::Result<()>>>>,
    }
    impl Launcher for Registered {
        fn start(&self) -> canonic_process_kit::Result<()> {
            self.count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            *self.running.lock().unwrap() = Some(thread_server(
                &self.root,
                &self.gate,
                Arc::new(Acme::default()),
                canonic_process_kit::server::Options::default(),
            ));
            Ok(())
        }
    }
    let (_temp, root) = root();
    let registered = Registered {
        root: root.clone(),
        gate: canonic_process_kit::core::Admission::new(),
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
