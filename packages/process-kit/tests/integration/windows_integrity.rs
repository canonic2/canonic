//! Windows processes at different integrity levels, and owner death, through
//! the public API. `integrity_role` performs one role in a process the
//! acceptance script starts elevated or at medium integrity; the owner test
//! re-executes it as a holding client and terminates that process.

use canonic_process_kit::{
    core::Admission,
    endpoint::{self, Lease, Root},
    owner,
};
use std::{
    fs,
    io::{Read, Write},
    path::PathBuf,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

const ROLE: &str = "windows_integrity::integrity_role";

/// This process's mandatory label as `whoami` reports it, for the result file.
fn label() -> String {
    let output = Command::new("whoami").arg("/groups").output().unwrap();
    // The line reads `Mandatory Label\High Mandatory Level ...`.
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .find_map(|line| line.split_once(r"Mandatory Label\"))
        .and_then(|(_, rest)| rest.split_whitespace().next())
        .unwrap_or("unknown")
        .to_owned()
}

fn variable(name: &str) -> PathBuf {
    PathBuf::from(std::env::var_os(name).unwrap())
}

/// One acceptance role, selected by `PROCESS_KIT_ROLE`: `prepare` creates the
/// root and anchor and exits; `serve` binds both lanes, answers one checked
/// peer on each and exits; `connect` reaches both lanes; `hold` connects and
/// waits to be terminated. A completed role writes `ok <label>` to
/// `PROCESS_KIT_RESULT`.
#[test]
#[ignore = "run by the Windows integrity acceptance script or the owner test"]
fn integrity_role() {
    let role = std::env::var("PROCESS_KIT_ROLE").unwrap();
    let root = Root::explicit(variable("PROCESS_KIT_ROOT")).unwrap();
    let result = variable("PROCESS_KIT_RESULT");
    match role.as_str() {
        "prepare" => {
            root.prepare().unwrap();
            drop(Lease::acquire(&root, Duration::ZERO).unwrap());
        }
        "serve" => {
            let lease = Lease::acquire(&root, Duration::from_secs(5)).unwrap();
            let endpoints = [lease.bind(false).unwrap(), lease.bind(true).unwrap()];
            fs::write(result.with_extension("ready"), "ready").unwrap();
            for endpoint in &endpoints {
                endpoint.listener().set_nonblocking(false).unwrap();
                let (mut stream, _) = endpoint.listener().accept().unwrap();
                endpoint::check_peer(&stream).unwrap();
                stream.write_all(b"k").unwrap();
                let mut end = [0];
                assert_eq!(stream.read(&mut end).unwrap(), 0);
            }
        }
        "connect" => {
            for control in [false, true] {
                let mut stream = endpoint::connect(&root, control, Duration::from_secs(5)).unwrap();
                let mut reply = [0];
                stream.read_exact(&mut reply).unwrap();
                assert_eq!(&reply, b"k");
            }
        }
        "hold" => {
            let _stream = endpoint::connect(&root, false, Duration::from_secs(5)).unwrap();
            fs::write(&result, "holding").unwrap();
            std::thread::sleep(Duration::from_mins(1));
        }
        other => panic!("unknown role {other}"),
    }
    fs::write(&result, format!("ok {}", label())).unwrap();
}

#[test]
fn terminated_owner_process_is_observed_as_eof_while_the_hold_remains() {
    let temporary = tempfile::tempdir().unwrap();
    let root = Root::explicit(temporary.path().join("acme")).unwrap();
    let lease = Lease::acquire(&root, Duration::ZERO).unwrap();
    let endpoint = lease.bind(false).unwrap();
    endpoint.listener().set_nonblocking(false).unwrap();
    let marker = temporary.path().join("holding");
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", ROLE, "--ignored", "--nocapture"])
        .env("PROCESS_KIT_ROLE", "hold")
        .env("PROCESS_KIT_ROOT", root.path())
        .env("PROCESS_KIT_RESULT", &marker)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let (stream, _) = endpoint.listener().accept().unwrap();
    let gate = Admission::new();
    let mut owner = owner::Connection::new(stream, gate.hold().unwrap()).unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while !marker.exists() {
        assert!(Instant::now() < deadline, "holding client did not connect");
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(!owner.wait(Duration::from_millis(50)).unwrap());

    child.kill().unwrap();
    child.wait().unwrap();
    assert!(owner.wait(Duration::from_secs(5)).unwrap());
    assert!(owner.wait(Duration::ZERO).unwrap());
    assert_eq!(gate.snapshot().held, 1);
    drop(owner);
    assert_eq!(gate.snapshot().held, 0);
    endpoint.unlink().unwrap();
}
