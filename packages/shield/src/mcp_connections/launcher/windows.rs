//! The private connector launcher owns a job before starting any connector code.
use std::{io, process::Command, sync::Mutex};
use win32job::{ExtendedLimitInfo, Job};

fn retain_job() -> io::Result<()> {
    // This private CLI process and all its descendants share one job. Keeping
    // its non-inheritable handle until OS exit also covers abrupt launcher loss.
    // Dropping it on the Rust return path would terminate the launcher itself.
    static JOB: Mutex<Option<Job>> = Mutex::new(None);
    let mut slot = JOB
        .lock()
        .map_err(|_| io::Error::other("connector job lock poisoned"))?;
    if slot.is_none() {
        let mut limits = ExtendedLimitInfo::new();
        limits.limit_kill_on_job_close();
        let job = Job::create_with_limit_info(&limits)
            .map_err(|error| io::Error::other(error.to_string()))?;
        job.assign_current_process()
            .map_err(|error| io::Error::other(error.to_string()))?;
        *slot = Some(job);
    }
    Ok(())
}

pub(super) fn run(mut command: Command) -> io::Result<u8> {
    retain_job()?;
    let mut child = command.spawn()?;
    match child.wait() {
        Ok(status) => {
            if !status.success() {
                eprintln!("Shield: MCP connector exited with {status}");
            }
            Ok(status
                .code()
                .and_then(|code| u8::try_from(code).ok())
                .unwrap_or(2))
        }
        Err(error) => {
            let _ = child.kill();
            let _ = child.wait();
            Err(error)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs,
        io::{Read, Write},
        path::Path,
        process::{Child, Stdio},
        time::{Duration, Instant},
    };
    use winsafe::{HPROCESS, co};

    const FIXTURE: &str = "mcp_connections::launcher::windows::tests::job_fixture";

    #[test]
    #[ignore = "owned subprocess of connector_job_ends_descendants_after_launcher_exit_or_loss"]
    fn job_fixture() {
        let root = std::env::var_os("SHIELD_TEST_JOB_ROOT").unwrap();
        let root = Path::new(&root);
        if std::env::var_os("SHIELD_TEST_JOB_DESCENDANT").is_some() {
            fs::write(root.join("descendant"), std::process::id().to_string()).unwrap();
            let deadline = Instant::now() + Duration::from_secs(15);
            while Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(10));
            }
            panic!("descendant survived its owner");
        }
        retain_job().unwrap();
        #[expect(
            clippy::zombie_processes,
            reason = "the parent verifies descendant termination after normal or forced loss of this Job owner"
        )]
        let _descendant = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", FIXTURE, "--ignored"])
            .env("SHIELD_TEST_JOB_DESCENDANT", "1")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        // The parent test either permits a normal return or kills this exact
        // process. In either case the OS closes the sole job handle.
        let _ = std::io::stdin().read(&mut [0_u8]);
    }

    struct Owner(Child);
    impl Drop for Owner {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }

    #[test]
    fn connector_job_ends_descendants_after_launcher_exit_or_loss() {
        for abrupt in [false, true] {
            let root = assert_fs::TempDir::new().unwrap();
            let mut owner = Owner(
                Command::new(std::env::current_exe().unwrap())
                    .args(["--exact", FIXTURE, "--ignored"])
                    .env("SHIELD_TEST_JOB_ROOT", root.path())
                    .stdin(Stdio::piped())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .spawn()
                    .unwrap(),
            );
            let deadline = Instant::now() + Duration::from_secs(10);
            let pid = loop {
                if let Some(pid) = fs::read_to_string(root.path().join("descendant"))
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
            if abrupt {
                owner.0.kill().unwrap();
            } else {
                owner.0.stdin.take().unwrap().write_all(b"exit").unwrap();
            }
            owner.0.wait().unwrap();
            assert_eq!(
                process.WaitForSingleObject(Some(5000)).unwrap(),
                co::WAIT::OBJECT_0
            );
        }
    }
}
