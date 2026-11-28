//! Ubuntu process scopes tied to a single owner pipe.

use std::{
    io,
    path::{Component, Path, PathBuf},
    process::Stdio,
    time::Duration,
};
use tokio::{
    process::{Child, Command},
    time::{self, Instant},
};

pub(super) struct Scope {
    anchor: Child,
    anchor_unit: String,
    work_unit: String,
    bus: String,
    events: PathBuf,
}

impl Scope {
    pub(super) async fn start(deadline: Instant) -> io::Result<Self> {
        // SAFETY: geteuid takes no parameters and the result names this user.
        let uid = unsafe { libc::geteuid() };
        let bus = format!("unix:path=/run/user/{uid}/bus");
        let id = uuid::Uuid::new_v4().simple().to_string();
        let anchor_unit = format!("testrun-browser-{id}-owner.scope");
        let work_unit = format!("testrun-browser-{id}.scope");
        let mut anchor = scope_command(&anchor_unit, &bus)
            .arg("--")
            .arg("/bin/cat")
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()?;
        let pid = anchor.id().ok_or_else(unavailable)?;
        let membership = format!("/proc/{pid}/cgroup");
        let events = loop {
            if let Ok(group) = tokio::fs::read_to_string(&membership).await
                && let Some(events) = work_events(&group, &anchor_unit, &work_unit)
            {
                break events;
            }
            if anchor.try_wait()?.is_some() || Instant::now() >= deadline {
                return Err(unavailable());
            }
            time::sleep_until(deadline.min(Instant::now() + Duration::from_millis(10))).await;
        };
        Ok(Self {
            anchor,
            anchor_unit,
            work_unit,
            bus,
            events,
        })
    }

    pub(super) fn command(&self, executable: &Path) -> Command {
        let mut command = scope_command(&self.work_unit, &self.bus);
        command
            .arg(format!("--property=BindsTo={}", self.anchor_unit))
            .arg(format!("--property=After={}", self.anchor_unit))
            // Graceful protocol shutdown precedes owner release. Any process
            // left in this private scope must exit within the cleanup budget.
            .arg("--property=KillSignal=SIGKILL")
            .args(["--", "/usr/bin/env", "-u", "DBUS_SESSION_BUS_ADDRESS"])
            .arg(executable);
        command
    }

    pub(super) fn end(&mut self) {
        // Only the capsule retains this pipe's writer. Kernel EOF also ends
        // the anchor when the owning Testrun process exits unexpectedly.
        self.anchor.stdin.take();
    }

    pub(super) async fn release(&mut self, deadline: Instant) -> io::Result<()> {
        self.end();
        time::timeout_at(deadline, self.anchor.wait()).await??;
        // The workload launcher has been reaped. The scope can no longer
        // receive its initial process, and the closing anchor stops survivors.
        loop {
            match tokio::fs::read_to_string(&self.events).await {
                Ok(events) if events.lines().any(|line| line == "populated 0") => return Ok(()),
                Ok(events) if events.lines().any(|line| line == "populated 1") => {}
                Ok(_) => {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "Invalid browser scope membership",
                    ));
                }
                Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
                // kernfs can deactivate an already opened cgroup.events file.
                // Reopen the path to confirm removal or read its current state.
                Err(error) if error.raw_os_error() == Some(libc::ENODEV) => {}
                Err(error) => return Err(error),
            }
            if Instant::now() >= deadline {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "Browser scope is still populated",
                ));
            }
            time::sleep_until(deadline.min(Instant::now() + Duration::from_millis(10))).await;
        }
    }
}

fn scope_command(unit: &str, bus: &str) -> Command {
    let mut command = Command::new("/usr/bin/systemd-run");
    command
        .env_clear()
        .env("DBUS_SESSION_BUS_ADDRESS", bus)
        .args([
            "--user",
            "--scope",
            "--quiet",
            "--slice=app.slice",
            "--expand-environment=no",
        ])
        .arg(format!("--unit={unit}"));
    command
}

fn work_events(membership: &str, anchor: &str, work: &str) -> Option<PathBuf> {
    let relative = Path::new(
        membership
            .lines()
            .find_map(|line| line.strip_prefix("0::/"))?,
    );
    if !relative
        .components()
        .all(|part| matches!(part, Component::Normal(_)))
        || relative.file_name()? != anchor
        || relative.parent()?.file_name()? != "app.slice"
    {
        return None;
    }
    Some(
        Path::new("/sys/fs/cgroup")
            .join(relative.parent()?)
            .join(work)
            .join("cgroup.events"),
    )
}

fn unavailable() -> io::Error {
    io::Error::other("The systemd user manager could not establish a browser scope")
}

#[cfg(all(test, feature = "acceptance-browser-driver"))]
mod tests {
    use super::*;
    use tokio::io::{AsyncBufReadExt, BufReader};

    #[tokio::test]
    async fn browser_scope_releases_a_separate_session_after_the_launcher_exits() {
        let deadline = Instant::now() + Duration::from_secs(15);
        let mut scope = Scope::start(deadline).await.unwrap();
        let mut child = scope
            .command(Path::new("/usr/bin/setsid"))
            .args([
                "--fork",
                "/bin/sh",
                "-c",
                "trap '' TERM; printf '%s\\n' \"$$\"; exec /bin/sleep 60",
            ])
            .stdout(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        let mut output = BufReader::new(child.stdout.take().unwrap()).lines();
        let pid = time::timeout_at(deadline, output.next_line())
            .await
            .unwrap()
            .unwrap()
            .unwrap()
            .parse::<u32>()
            .unwrap();
        assert!(
            time::timeout_at(deadline, child.wait())
                .await
                .unwrap()
                .unwrap()
                .success()
        );
        let membership = tokio::fs::read_to_string(format!("/proc/{pid}/cgroup"))
            .await
            .unwrap();
        assert!(
            membership
                .lines()
                .any(|line| line.ends_with(&scope.work_unit))
        );
        assert!(
            tokio::fs::read_to_string(&scope.events)
                .await
                .unwrap()
                .lines()
                .any(|line| line == "populated 1")
        );

        scope.release(deadline).await.unwrap();
        assert!(
            super::super::linux::info(pid)
                .unwrap()
                .is_none_or(|info| info.zombie)
        );
    }
}
