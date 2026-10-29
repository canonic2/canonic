use super::*;
fn request() -> policy::Request {
    policy::Request {
        executable: "/bin/sleep".into(),
        arguments: vec!["10".into()],
        cwd: "/private/tmp".into(),
        project: "/private/tmp".into(),
    }
}
#[test]
fn timeout_and_drop_kill_only_owned_children_and_reap_them() {
    let signals = Signals {
        flag: Arc::new(AtomicUsize::new(0)),
        ids: vec![],
    };
    let mut child = OwnedChild::spawn(&request()).unwrap();
    let pid = Pid::from_raw(i32::try_from(child.child.id()).unwrap());
    assert_eq!(child.wait(&signals, Instant::now(), 20).unwrap().code, 124);
    assert_eq!(killpg(pid, None), Err(nix::errno::Errno::ESRCH));
    let child = OwnedChild::spawn(&request()).unwrap();
    let pid = Pid::from_raw(i32::try_from(child.child.id()).unwrap());
    drop(child);
    assert_eq!(killpg(pid, None), Err(nix::errno::Errno::ESRCH));
}
#[test]
fn cancellation_forwards_signal_and_reports_it() {
    let signals = Signals {
        flag: Arc::new(AtomicUsize::new(signal_hook::consts::SIGINT as usize)),
        ids: vec![],
    };
    let mut child = OwnedChild::spawn(&request()).unwrap();
    let outcome = child.wait(&signals, Instant::now(), 5000).unwrap();
    assert_eq!(outcome.code, 130);
    assert_eq!(outcome.signal, Some(2));
    assert_eq!(outcome.status, "cancelled");
}

#[test]
fn ignored_term_escalates_to_kill_and_reaps() {
    let root = assert_fs::TempDir::new().unwrap();
    let ready = root.path().join("ready");
    let mut request = request();
    request.executable = "/bin/sh".into();
    request.arguments = vec![
        "-c".into(),
        "trap '' TERM; /bin/sleep 30 & : > \"$1\"; wait".into(),
        "acme".into(),
        ready.to_str().unwrap().into(),
    ];
    let mut child = OwnedChild::spawn(&request).unwrap();
    let start = Instant::now();
    while !ready.exists() {
        assert!(start.elapsed() < Duration::from_secs(3));
        std::thread::sleep(Duration::from_millis(10));
    }
    let signals = Signals {
        flag: Arc::new(AtomicUsize::new(0)),
        ids: vec![],
    };
    let outcome = child.wait(&signals, Instant::now(), 20).unwrap();
    assert_eq!(outcome.signal, Some(9));
    assert_eq!(outcome.code, 124);
}

// These test-only shell fixtures exercise the private process owner. Shells
// remain unsupported at the checked native launch boundary.
fn tree(root: &std::path::Path, exit_leader: bool) -> OwnedChild {
    let mut input = request();
    input.executable = "/bin/sh".into();
    input.arguments = vec![
        "-c".into(),
        concat!(
            "trap 'exit 0' TERM INT HUP; ",
            "/bin/sh -c 'trap \"\" TERM INT HUP; ",
            "/bin/sleep 30 & echo $! > \"$1/grandchild\"; ",
            "echo $$ > \"$1/descendant\"; : > \"$1/ready\"; wait' acme \"$1\" & ",
            "while [ ! -e \"$1/ready\" ]; do /bin/sleep 0.01; done; ",
            ": > \"$1/leader-ready\"; ",
            "if [ \"$2\" = exit ]; then exit 7; fi; wait"
        )
        .into(),
        "acme".into(),
        root.to_str().expect("temporary path").into(),
        if exit_leader { "exit" } else { "wait" }.into(),
    ];
    let child = OwnedChild::spawn(&input).expect("owned process tree");
    let start = Instant::now();
    while !root.join("leader-ready").exists() {
        assert!(start.elapsed() < Duration::from_secs(3), "tree ready");
        std::thread::sleep(Duration::from_millis(10));
    }
    child
}

fn assert_tree_gone(root: &std::path::Path) {
    use nix::sys::signal::kill;
    for name in ["descendant", "grandchild"] {
        let pid = Pid::from_raw(
            std::fs::read_to_string(root.join(name))
                .expect("owned PID")
                .trim()
                .parse()
                .expect("PID integer"),
        );
        let start = Instant::now();
        while kill(pid, None) != Err(nix::errno::Errno::ESRCH) {
            assert!(start.elapsed() < Duration::from_secs(3), "{name} survived");
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

#[test]
fn group_cleanup_survives_leader_exit_and_preserves_unrelated_child() {
    let root = assert_fs::TempDir::new().expect("temporary tree");
    let unrelated = OwnedChild::spawn(&request()).expect("separate group");
    let mut child = tree(root.path(), true);
    let signals = Signals {
        flag: Arc::new(AtomicUsize::new(0)),
        ids: vec![],
    };
    let outcome = child
        .wait(&signals, Instant::now(), 5000)
        .expect("tree completion");
    assert_eq!(outcome.code, 7);
    assert_eq!(outcome.exit_code, Some(7));
    assert_eq!(outcome.status, "failed");
    assert_tree_gone(root.path());
    assert!(!unrelated.exited().expect("unrelated still running"));
    assert!(
        child.send(Signal::SIGKILL).is_err(),
        "reaped group must never be signalled"
    );
}

#[test]
fn leader_exit_during_timeout_and_each_interrupt_still_kills_ignoring_descendants() {
    for signal in [
        0,
        signal_hook::consts::SIGINT,
        signal_hook::consts::SIGTERM,
        signal_hook::consts::SIGHUP,
    ] {
        let root = assert_fs::TempDir::new().expect("temporary tree");
        let mut child = tree(root.path(), false);
        let signals = Signals {
            flag: Arc::new(AtomicUsize::new(usize::try_from(signal).expect("signal"))),
            ids: vec![],
        };
        let outcome = child
            .wait(
                &signals,
                Instant::now(),
                if signal == 0 { 20 } else { 5000 },
            )
            .expect("tree completion");
        assert_eq!(
            outcome.code,
            if signal == 0 {
                124
            } else {
                u8::try_from(128 + signal).expect("code")
            }
        );
        assert_eq!(
            outcome.exit_code,
            Some(0),
            "leader handles signal and exits before KILL"
        );
        assert_tree_gone(root.path());
    }
}

#[test]
fn abort_and_drop_clean_group_even_after_leader_exit() {
    for abort in [true, false] {
        let root = assert_fs::TempDir::new().expect("temporary tree");
        let mut child = tree(root.path(), true);
        let start = Instant::now();
        while !child.exited().expect("unreaped leader status") {
            assert!(start.elapsed() < Duration::from_secs(3));
            std::thread::sleep(Duration::from_millis(10));
        }
        if abort {
            let result = child.abort().expect("abort tree");
            assert_eq!(result.status, "aborted-after-launch");
            assert_eq!(result.exit_code, Some(7));
        }
        drop(child);
        assert_tree_gone(root.path());
    }
}

#[test]
fn moved_leader_cleanup_preserves_destination_group_and_reaps_exact_child() {
    for with_descendant in [false, true] {
        moved_leader_cleanup(with_descendant);
    }
}

fn moved_leader_cleanup(with_descendant: bool) {
    for mode in [
        "timeout",
        "int",
        "term",
        "hup",
        "abort",
        "drop",
        "exit",
        "exited-abort",
    ] {
        let root = assert_fs::TempDir::new().expect("owned leader fixture");
        let unrelated = OwnedChild::spawn(&request()).expect("destination group");
        std::fs::write(root.path().join("leader-group"), unrelated.id().to_string())
            .expect("owned destination identity");
        if with_descendant {
            std::fs::write(root.path().join("with-descendant"), b"").expect("tree mode");
        }
        let mut child = moved_leader(root.path());
        let pid = Pid::from_raw(i32::try_from(child.id()).expect("leader PID"));
        assert_eq!(
            nix::unistd::getpgid(Some(pid))
                .expect("moved group")
                .as_raw(),
            i32::try_from(unrelated.id()).expect("destination PID")
        );
        if matches!(mode, "exit" | "exited-abort") {
            std::fs::write(root.path().join("stop"), b"").expect("normal fixture exit");
            let start = Instant::now();
            while !child.exited().expect("unreaped exit") {
                assert!(start.elapsed() < Duration::from_secs(3));
                std::thread::sleep(Duration::from_millis(10));
            }
        }
        match mode {
            "drop" => drop(child),
            "abort" | "exited-abort" => {
                let outcome = child.abort().expect("abort moved leader");
                assert_eq!(outcome.status, "aborted-after-launch");
                assert_eq!(outcome.exit_code, (mode == "exited-abort").then_some(7));
                assert_eq!(outcome.signal, (mode == "abort").then_some(9));
                assert!(child.send(Signal::SIGKILL).is_err());
            }
            _ => assert_moved_leader_wait(&mut child, mode, root.path()),
        }
        assert_eq!(
            nix::sys::signal::kill(pid, None),
            Err(nix::errno::Errno::ESRCH)
        );
        assert!(
            matches!(
                waitid(
                    WaitId::Pid(rustix::process::Pid::from_raw(pid.as_raw()).expect("PID")),
                    WaitIdOptions::EXITED | WaitIdOptions::NOHANG | WaitIdOptions::NOWAIT
                ),
                Err(rustix::io::Errno::CHILD)
            ),
            "owner reaped the exact direct child"
        );
        assert!(
            !unrelated
                .exited()
                .expect("destination group's original member survives")
        );
        if with_descendant {
            let descendant = Pid::from_raw(
                std::fs::read_to_string(root.path().join("descendant"))
                    .expect("owned descendant")
                    .parse()
                    .expect("PID"),
            );
            let start = Instant::now();
            while kill(descendant, None) != Err(nix::errno::Errno::ESRCH) {
                assert!(
                    start.elapsed() < Duration::from_secs(3),
                    "original-group descendant survived"
                );
                std::thread::sleep(Duration::from_millis(10));
            }
        }
    }
}

fn assert_moved_leader_wait(child: &mut OwnedChild, mode: &str, root: &std::path::Path) {
    let signal = match mode {
        "int" => signal_hook::consts::SIGINT,
        "term" => signal_hook::consts::SIGTERM,
        "hup" => signal_hook::consts::SIGHUP,
        _ => 0,
    };
    let signals = Signals {
        flag: Arc::new(AtomicUsize::new(usize::try_from(signal).expect("signal"))),
        ids: vec![],
    };
    let start = Instant::now();
    let outcome = child
        .wait(&signals, start, if mode == "timeout" { 20 } else { 5000 })
        .expect("moved leader completion");
    if mode == "exit" {
        assert_eq!(outcome.code, 7);
        assert_eq!(outcome.exit_code, Some(7));
        assert_eq!(outcome.status, "failed");
    } else {
        assert!(start.elapsed() >= Duration::from_millis(500));
        assert_eq!(
            outcome.signal,
            Some(9),
            "ignored signal requires escalation"
        );
        assert_eq!(outcome.exit_code, None);
        assert_eq!(
            outcome.code,
            if signal == 0 {
                124
            } else {
                u8::try_from(128 + signal).expect("signal exit code")
            }
        );
        assert_eq!(
            outcome.status,
            if signal == 0 {
                "timed-out"
            } else {
                "cancelled"
            }
        );
        assert_eq!(
            std::fs::read_to_string(root.join("received")).expect("forwarded signal"),
            if signal == 0 {
                signal_hook::consts::SIGTERM
            } else {
                signal
            }
            .to_string()
        );
    }
    assert!(child.send(Signal::SIGKILL).is_err());
}

fn moved_leader(root: &std::path::Path) -> OwnedChild {
    let mut input = request();
    input.executable = std::env::current_exe()
        .expect("test executable")
        .to_str()
        .expect("path")
        .into();
    input.cwd = root.to_str().expect("root").into();
    input.arguments = vec![
        "--exact".into(),
        "execution::process::tests::moved_leader_fixture".into(),
        "--ignored".into(),
    ];
    assert!(
        !super::super::supported(&input),
        "harness remains blocked by public binding"
    );
    let child = OwnedChild::spawn(&input).expect("owned moving leader");
    let start = Instant::now();
    while !root.join("leader-ready").exists() {
        assert!(start.elapsed() < Duration::from_secs(3), "leader ready");
        std::thread::sleep(Duration::from_millis(10));
    }
    child
}

#[test]
#[ignore = "owned subprocess harness invoked by moved-leader acceptance"]
fn moved_leader_fixture() {
    let root = std::env::current_dir().expect("fixture cwd");
    let destination = std::fs::read_to_string(root.join("leader-group"))
        .expect("owned fixture marker")
        .parse::<i32>()
        .expect("destination group");
    let _descendant = root.join("with-descendant").exists().then(|| {
        let child = Command::new("/bin/sleep")
            .arg("10")
            .spawn()
            .expect("owned original-group child");
        std::fs::write(root.join("descendant"), child.id().to_string()).expect("descendant PID");
        child
    });
    let flag = Arc::new(AtomicUsize::new(0));
    let mut ids = vec![];
    for signal in [
        signal_hook::consts::SIGINT,
        signal_hook::consts::SIGTERM,
        signal_hook::consts::SIGHUP,
    ] {
        ids.push(
            signal_hook::flag::register_usize(
                signal,
                Arc::clone(&flag),
                usize::try_from(signal).expect("signal"),
            )
            .expect("ignore fixture interruption"),
        );
    }
    nix::unistd::setpgid(Pid::from_raw(0), Pid::from_raw(destination))
        .expect("join another owned group in the same session");
    std::fs::write(root.join("leader-ready"), b"").expect("fixture ready");
    let start = Instant::now();
    let mut recorded = false;
    while !root.join("stop").exists() && start.elapsed() < Duration::from_secs(10) {
        let received = flag.load(Ordering::SeqCst);
        if received != 0 && !recorded {
            std::fs::write(root.join("received.tmp"), received.to_string())
                .expect("signal evidence");
            std::fs::rename(root.join("received.tmp"), root.join("received"))
                .expect("publish signal evidence");
            recorded = true;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    // Bound fixture lifetime even if the process owner regresses.
    std::process::exit(7);
}

struct EscapeCleanup(std::path::PathBuf);
impl Drop for EscapeCleanup {
    fn drop(&mut self) {
        std::fs::write(self.0.join("stop"), b"").expect("release owned escape worker");
        let start = Instant::now();
        while !self.0.join("stopped").exists() && start.elapsed() < Duration::from_secs(3) {
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

#[test]
fn changed_group_is_outside_containment_and_remains_fail_closed_at_launch() {
    for kind in ["group", "session"] {
        escaped_tree(kind, false);
    }
}

#[test]
fn escape_fixture_startup_is_separate_from_cleanup_deadlines() {
    escaped_tree("group", true);
}

fn escaped_tree(kind: &str, delayed_start: bool) {
    let root = assert_fs::TempDir::new().expect("escape root");
    std::fs::write(root.path().join("escape-fixture"), kind).expect("owned marker");
    if delayed_start {
        std::fs::write(root.path().join("delayed-start"), b"").expect("startup regression");
    }
    let cleanup = EscapeCleanup(root.path().into());
    let mut input = request();
    input.executable = std::env::current_exe()
        .expect("test executable")
        .to_str()
        .expect("path")
        .into();
    input.cwd = root.path().to_str().expect("root path").into();
    input.arguments = vec![
        "--exact".into(),
        "execution::process::tests::escape_fixture".into(),
        "--ignored".into(),
    ];
    assert!(
        !super::super::supported(&input),
        "test harness cannot cross the checked launch boundary"
    );
    let mut child = OwnedChild::spawn(&input).expect("owned fixture leader");
    let start = Instant::now();
    while !root.path().join("escaped-ready").exists() {
        // Harness readiness is a precondition, not a launch-latency guarantee.
        // Keep this setup budget separate from the three-second cleanup checks.
        assert!(
            start.elapsed() < Duration::from_secs(30),
            "escape fixture startup"
        );
        assert!(
            !child.exited().expect("fixture status"),
            "fixture exited before readiness"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    let escaped = std::fs::read_to_string(root.path().join("escaped-pid"))
        .expect("escaped PID")
        .parse::<i32>()
        .expect("PID");
    assert_ne!(
        nix::unistd::getpgid(Some(Pid::from_raw(escaped)))
            .expect("new group")
            .as_raw(),
        i32::try_from(child.id()).expect("leader PID")
    );
    child.abort().expect("original group cleanup");
    assert!(
        nix::sys::signal::kill(Pid::from_raw(escaped), None).is_ok(),
        "escaped descendant survives original group cleanup"
    );
    drop(cleanup);
    let start = Instant::now();
    while nix::sys::signal::kill(Pid::from_raw(escaped), None) != Err(nix::errno::Errno::ESRCH) {
        assert!(
            start.elapsed() < Duration::from_secs(3),
            "owned escape worker must exit"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
#[ignore = "owned subprocess harness invoked by the escape acceptance test"]
fn escape_fixture() {
    let root = std::env::current_dir().expect("fixture cwd");
    assert!(
        root.join("escape-fixture").exists(),
        "only an owned fixture may run"
    );
    if let Ok(kind) = std::env::var("SHIELD_TEST_ESCAPE_WORKER") {
        if kind == "session" {
            nix::unistd::setsid().expect("owned new session");
        }
        if root.join("delayed-start").exists() {
            std::thread::sleep(Duration::from_millis(3100));
        }
        std::fs::write(root.join("escaped-pid"), std::process::id().to_string())
            .expect("worker identity");
        std::fs::write(root.join("escaped-ready"), b"").expect("worker ready");
        let start = Instant::now();
        while !root.join("stop").exists() && start.elapsed() < Duration::from_secs(10) {
            std::thread::sleep(Duration::from_millis(10));
        }
        std::fs::write(root.join("stopped"), b"").expect("worker stopped");
    } else {
        let kind = std::fs::read_to_string(root.join("escape-fixture")).expect("fixture kind");
        let mut command = Command::new(std::env::current_exe().expect("test binary"));
        command
            .args([
                "--exact",
                "execution::process::tests::escape_fixture",
                "--ignored",
            ])
            .env("SHIELD_TEST_ESCAPE_WORKER", &kind)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        if kind == "group" {
            command.process_group(0);
        }
        let mut worker = command.spawn().expect("owned escaped worker");
        worker
            .wait()
            .expect("reap worker if fixture is not interrupted");
    }
}
