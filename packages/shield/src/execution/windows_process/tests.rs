//! Native Job lifecycle checks keep exact owned process handles through cleanup.
use super::*;
use std::{
    fs,
    io::{Read, Write},
    path::Path,
};
use winsafe::{HPROCESS, co};

const FIXTURE: &str = "execution::windows_process::tests::windows_owned_child_fixture";
const LOADER_FIXTURE: &str = "execution::windows_process::tests::windows_owned_loader_fixture";

fn fixture(root: &Path, mode: &str) -> Command {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args(["--exact", FIXTURE, "--ignored", "--nocapture"])
        .env("SHIELD_WINDOWS_RUN_ROOT", root)
        .env("SHIELD_WINDOWS_RUN_MODE", mode)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    command
}

#[test]
#[ignore = "owned child of Windows execution lifecycle fixtures"]
fn windows_owned_child_fixture() {
    let directory = std::env::var_os("SHIELD_WINDOWS_RUN_ROOT").unwrap();
    let root = Path::new(&directory);
    match std::env::var("SHIELD_WINDOWS_RUN_MODE").unwrap().as_str() {
        "wait" => {
            fs::write(root.join("ready"), std::process::id().to_string()).unwrap();
            let deadline = Instant::now() + Duration::from_secs(15);
            while Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(10));
            }
            panic!("owned child outlived its fixture");
        }
        "fork" => {
            #[expect(
                clippy::zombie_processes,
                reason = "the parent fixture verifies kernel Job cleanup after this owner exits without waiting"
            )]
            let _child = fixture(root, "wait").spawn().unwrap();
            let _ = io::stdin().read(&mut [0]);
        }
        "owner" => {
            let _child = OwnedChild::from_command(fixture(root, "wait")).unwrap();
            let _ = io::stdin().read(&mut [0]);
            // Bypass Rust destructors: kernel handle closure owns this cleanup.
            std::process::exit(0);
        }
        "fail" => std::process::exit(7),
        "loader-owner" => {
            static JOB: std::sync::Mutex<Option<win32job::Job>> = std::sync::Mutex::new(None);
            let mut limits = win32job::ExtendedLimitInfo::new();
            limits.limit_kill_on_job_close();
            let job = win32job::Job::create_with_limit_info(&limits).unwrap();
            job.assign_current_process().unwrap();
            *JOB.lock().unwrap() = Some(job);
            winsafe::SetDllDirectory(None).unwrap();
            let system = PathBuf::from(winsafe::GetSystemDirectory().unwrap());
            fs::copy(
                system.join("version.dll"),
                root.join("acme-owned-loader.dll"),
            )
            .unwrap();
            let executable = std::env::current_exe().unwrap();
            let arguments = ["--exact", LOADER_FIXTURE, "--ignored", "--nocapture"];
            fs::write(root.join("loader-expectation"), "found").unwrap();
            assert!(
                Command::new(&executable)
                    .args(arguments)
                    .current_dir(root)
                    .env_clear()
                    .env("SystemRoot", system.parent().unwrap())
                    .status()
                    .unwrap()
                    .success()
            );
            fs::write(root.join("loader-expectation"), "missing").unwrap();
            let request = policy::Request {
                executable: executable.to_str().unwrap().into(),
                arguments: arguments.map(str::to_owned).to_vec(),
                cwd: root.to_str().unwrap().into(),
                project: root.to_str().unwrap().into(),
            };
            let signals = Signals::new().unwrap();
            let mut child = OwnedChild::spawn(&request).unwrap();
            assert_eq!(child.wait(&signals, Instant::now(), 5000).unwrap().code, 0);
        }
        "normal" => {}
        _ => panic!("invalid owned mode"),
    }
}

#[test]
#[ignore = "owned loader child of the isolated DLL-search fixture"]
fn windows_owned_loader_fixture() {
    let expected = fs::read_to_string("loader-expectation").unwrap();
    let found = winsafe::HINSTANCE::LoadLibrary("acme-owned-loader.dll").is_ok();
    fs::write("loader-observed", format!("{expected}:{found}")).unwrap();
    assert_eq!(
        found,
        expected == "found",
        "inherited DLL search policy: {expected}"
    );
}

#[test]
fn windows_owned_child_removes_project_cwd_from_inherited_dll_search() {
    let root = assert_fs::TempDir::new().unwrap();
    let mut owner = Owner(fixture(root.path(), "loader-owner").spawn().unwrap());
    let deadline = Instant::now() + Duration::from_secs(10);
    let status = loop {
        if let Some(status) = owner.0.try_wait().unwrap() {
            break status;
        }
        assert!(
            Instant::now() < deadline,
            "owned loader probe did not finish"
        );
        std::thread::sleep(Duration::from_millis(10));
    };
    assert!(
        status.success(),
        "native DLL search contract: {status}; stage {:?}",
        fs::read_to_string(root.path().join("loader-observed"))
    );
}

fn descendant(root: &Path) -> winsafe::guard::CloseHandleGuard<HPROCESS> {
    let deadline = Instant::now() + Duration::from_secs(10);
    let pid = loop {
        if let Some(pid) = fs::read_to_string(root.join("ready"))
            .ok()
            .and_then(|text| text.parse::<u32>().ok())
        {
            break pid;
        }
        assert!(
            Instant::now() < deadline,
            "owned descendant did not become ready"
        );
        std::thread::sleep(Duration::from_millis(10));
    };
    let process = HPROCESS::OpenProcess(co::PROCESS::SYNCHRONIZE, false, pid).unwrap();
    assert_eq!(
        process.WaitForSingleObject(Some(0)).unwrap(),
        co::WAIT::TIMEOUT
    );
    process
}

#[test]
fn windows_owned_job_cleans_normal_failure_timeout_abort_drop_and_descendants() {
    let signals = Signals::new().unwrap();
    for (mode, expected) in [("normal", 0), ("fail", 7)] {
        let root = assert_fs::TempDir::new().unwrap();
        let mut child = OwnedChild::from_command(fixture(root.path(), mode)).unwrap();
        assert_ne!(child.id(), std::process::id());
        let outcome = child.wait(&signals, Instant::now(), 5000).unwrap();
        assert_eq!(outcome.code, expected);
        assert_eq!(
            outcome.status,
            if expected == 0 { "completed" } else { "failed" }
        );
        assert_eq!(outcome.exit_code, Some(i32::from(expected)));
        assert_eq!(outcome.signal, None);
    }
    for mode in ["normal-parent", "timeout", "abort", "drop"] {
        let root = assert_fs::TempDir::new().unwrap();
        let mut child = OwnedChild::from_command(fixture(root.path(), "fork")).unwrap();
        let process = descendant(root.path());
        match mode {
            "normal-parent" => {
                child
                    .child
                    .stdin()
                    .take()
                    .unwrap()
                    .write_all(b"finish")
                    .unwrap();
                assert_eq!(child.wait(&signals, Instant::now(), 5000).unwrap().code, 0);
            }
            "timeout" => {
                let outcome = child.wait(&signals, Instant::now(), 1).unwrap();
                assert_eq!(outcome.code, 124);
                assert_eq!(outcome.status, "timed-out");
            }
            "abort" => {
                let outcome = child.abort().unwrap();
                assert_eq!(outcome.code, 2);
                assert_eq!(outcome.status, "aborted-after-launch");
            }
            "drop" => drop(child),
            _ => unreachable!(),
        }
        assert_eq!(
            process.WaitForSingleObject(Some(1000)).unwrap(),
            co::WAIT::OBJECT_0,
            "{mode}"
        );
    }
}

struct Owner(Child);
impl Drop for Owner {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
fn windows_owned_job_ends_children_after_owner_exit_and_forced_loss() {
    for abrupt in [false, true] {
        let root = assert_fs::TempDir::new().unwrap();
        let mut owner = Owner(fixture(root.path(), "owner").spawn().unwrap());
        let process = descendant(root.path());
        if abrupt {
            owner.0.kill().unwrap();
        } else {
            owner.0.stdin.take().unwrap().write_all(b"finish").unwrap();
        }
        let deadline = Instant::now() + Duration::from_secs(5);
        while owner.0.try_wait().unwrap().is_none() {
            assert!(Instant::now() < deadline, "owned parent did not exit");
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(
            process.WaitForSingleObject(Some(1000)).unwrap(),
            co::WAIT::OBJECT_0
        );
    }
}
