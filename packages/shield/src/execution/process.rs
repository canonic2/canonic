//! Owns the direct child and original group, signals, escalation and reaping.
use super::Error;
use crate::policy;
use nix::{
    sys::signal::{Signal, kill, killpg},
    unistd::Pid,
};
use rustix::process::{WaitId, WaitIdOptions, waitid};
use std::{
    os::unix::process::{CommandExt, ExitStatusExt},
    process::{Child, Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

pub(super) struct Signals {
    flag: Arc<AtomicUsize>,
    ids: Vec<signal_hook::SigId>,
}
impl Signals {
    pub(super) fn new() -> Result<Self, Error> {
        let mut value = Self {
            flag: Arc::new(AtomicUsize::new(0)),
            ids: vec![],
        };
        for signal in [
            signal_hook::consts::SIGINT,
            signal_hook::consts::SIGTERM,
            signal_hook::consts::SIGHUP,
        ] {
            value.ids.push(signal_hook::flag::register_usize(
                signal,
                Arc::clone(&value.flag),
                usize::try_from(signal).map_err(|_| Error::Unsupported)?,
            )?);
        }
        Ok(value)
    }
    pub(super) fn interrupted(&self) -> bool {
        self.flag.load(Ordering::SeqCst) != 0
    }
    pub(super) fn exit_code(&self) -> Option<u8> {
        self.signal()
            .and_then(|signal| u8::try_from(128 + signal as i32).ok())
    }
    fn signal(&self) -> Option<Signal> {
        i32::try_from(self.flag.load(Ordering::SeqCst))
            .ok()
            .and_then(|n| Signal::try_from(n).ok())
    }
}
impl Drop for Signals {
    fn drop(&mut self) {
        for id in self.ids.drain(..) {
            signal_hook::low_level::unregister(id);
        }
    }
}

pub(super) struct Outcome {
    pub(super) code: u8,
    pub(super) status: &'static str,
    pub(super) exit_code: Option<i32>,
    pub(super) signal: Option<i32>,
}
pub(super) struct OwnedChild {
    child: Child,
    finished: bool,
}
impl OwnedChild {
    pub(super) fn id(&self) -> u32 {
        self.child.id()
    }
    pub(super) fn abort(&mut self) -> Result<Outcome, Error> {
        self.finish_owned()?;
        let status = self.child.wait()?;
        self.finished = true;
        Ok(Outcome {
            code: 2,
            status: "aborted-after-launch",
            exit_code: status.code(),
            signal: status.signal(),
        })
    }
    pub(super) fn spawn(request: &policy::Request) -> Result<Self, Error> {
        let mut command = Command::new(&request.executable);
        command.env_clear();
        if request.executable == super::git::EXECUTABLE {
            command
                .env("GIT_CONFIG_GLOBAL", "/dev/null")
                .env("GIT_CONFIG_SYSTEM", "/dev/null")
                .env("GIT_OPTIONAL_LOCKS", "0")
                .env("GIT_TERMINAL_PROMPT", "0")
                .env("GIT_NO_LAZY_FETCH", "1");
            command.env("GIT_ATTR_NOSYSTEM", "1").env(
                "GIT_ATTR_SOURCE",
                "4b825dc642cb6eb9a060e54bf8d69288fbee4904",
            );
            command
                .env(
                    "GIT_DIR",
                    std::path::Path::new(&request.project).join(".git"),
                )
                .env("GIT_WORK_TREE", &request.project);
        }
        let child = command
            .args(&request.arguments)
            .current_dir(&request.cwd)
            .env("LC_ALL", "C")
            .stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .process_group(0)
            .spawn()?;
        Ok(Self {
            child,
            finished: false,
        })
    }
    fn send(&self, signal: Signal) -> Result<(), Error> {
        // Never signal a numeric group after reaping its leader: the PID/PGID
        // may have been reused. This guard is the sole waiter for that child.
        if self.finished {
            return Err(Error::Changed);
        }
        // Attempt both destinations even if one fails. The direct child can
        // leave its original group; never signal the group it subsequently joins.
        // A child still in the original group may receive the signal twice.
        let child = self.send_child(signal);
        let group = self.send_group(signal);
        child.and(group)
    }
    fn send_child(&self, signal: Signal) -> Result<(), Error> {
        let pid = Pid::from_raw(i32::try_from(self.child.id()).map_err(|_| Error::Changed)?);
        match kill(pid, signal) {
            Ok(()) | Err(nix::errno::Errno::ESRCH) => Ok(()),
            // Like group signalling, Darwin can reject a zombie. Only a
            // non-reaping wait on this exact child establishes its exit.
            Err(nix::errno::Errno::EPERM) if self.exited()? => Ok(()),
            Err(error) => Err(std::io::Error::from(error).into()),
        }
    }
    fn send_group(&self, signal: Signal) -> Result<(), Error> {
        let pid = Pid::from_raw(i32::try_from(self.child.id()).map_err(|_| Error::Changed)?);
        match killpg(pid, signal) {
            Ok(()) | Err(nix::errno::Errno::ESRCH) => Ok(()),
            // Darwin returns EPERM for an existing group containing only
            // zombies. Do not confuse that with permission to ignore a real
            // signal failure: require an empty live-group observation.
            Err(nix::errno::Errno::EPERM) if self.group_quiescent()? => Ok(()),
            Err(e) => Err(std::io::Error::from(e).into()),
        }
    }
    fn group_quiescent(&self) -> Result<bool, Error> {
        use libproc::processes::{ProcFilter, pids_by_type};
        // libproc's safe wrapper distinguishes an empty list from failure via
        // errno, but does not clear a prior syscall's errno itself. A missing
        // original group after leader movement is a valid empty observation.
        nix::errno::Errno::clear();
        let members = pids_by_type(ProcFilter::ByProgramGroup {
            pgrpid: self.child.id(),
        })?;
        for pid in members {
            let pid = Pid::from_raw(i32::try_from(pid).map_err(|_| Error::Changed)?);
            // Darwin getpgid excludes zombies. Recheck membership because a
            // listed PID may exit or move to another group before inspection.
            match nix::unistd::getpgid(Some(pid)) {
                Ok(group) if u32::try_from(group.as_raw()).ok() == Some(self.child.id()) => {
                    return Ok(false);
                }
                Ok(_) | Err(nix::errno::Errno::ESRCH) => {}
                Err(error) => return Err(std::io::Error::from(error).into()),
            }
        }
        Ok(true)
    }
    fn exited(&self) -> Result<bool, Error> {
        let pid = rustix::process::Pid::from_raw(
            i32::try_from(self.child.id()).map_err(|_| Error::Changed)?,
        )
        .ok_or(Error::Changed)?;
        match waitid(
            WaitId::Pid(pid),
            WaitIdOptions::EXITED | WaitIdOptions::NOHANG | WaitIdOptions::NOWAIT,
        ) {
            Ok(status) => Ok(status.is_some()),
            Err(rustix::io::Errno::INTR) => Ok(false),
            Err(error) => Err(std::io::Error::from(error).into()),
        }
    }
    fn finish_owned(&self) -> Result<(), Error> {
        let started = Instant::now();
        loop {
            self.send(Signal::SIGKILL)?;
            if self.group_quiescent()? && self.exited()? {
                return Ok(());
            }
            if started.elapsed() >= Duration::from_millis(500) {
                return Err(Error::CleanupIncomplete);
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }
    pub(super) fn wait(
        &mut self,
        signals: &Signals,
        started: Instant,
        timeout: u64,
    ) -> Result<Outcome, Error> {
        let mut stopping = None;
        loop {
            // Observe exit without reaping so the original leader still reserves
            // its PID while final group cleanup runs. Leader exit during a grace
            // period must not skip KILL for descendants that ignored the signal.
            if self.exited()?
                && stopping.is_none_or(|(time, _, _): (Instant, &str, u8)| {
                    time.elapsed() >= Duration::from_millis(500)
                })
            {
                self.finish_owned()?;
                let status = self.child.wait()?;
                self.finished = true;
                let (kind, code) = stopping.map_or_else(
                    || {
                        (
                            if status.success() {
                                "completed"
                            } else {
                                "failed"
                            },
                            status
                                .code()
                                .and_then(|n| u8::try_from(n).ok())
                                .unwrap_or_else(|| {
                                    u8::try_from(128 + status.signal().unwrap_or(0)).unwrap_or(2)
                                }),
                        )
                    },
                    |(_, kind, code)| (kind, code),
                );
                return Ok(Outcome {
                    code,
                    status: kind,
                    exit_code: status.code(),
                    signal: status.signal(),
                });
            }
            if let Some((time, _, _)) = stopping {
                if Instant::now().duration_since(time) >= Duration::from_millis(500) {
                    self.send(Signal::SIGKILL)?;
                }
            } else if let Some(signal) = signals.signal() {
                self.send(signal)?;
                stopping = Some((
                    Instant::now(),
                    "cancelled",
                    u8::try_from(128 + signal as i32).unwrap_or(2),
                ));
            } else if started.elapsed().as_millis() >= u128::from(timeout) {
                self.send(Signal::SIGTERM)?;
                stopping = Some((Instant::now(), "timed-out", 124));
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}
impl Drop for OwnedChild {
    fn drop(&mut self) {
        if !self.finished {
            if let Err(error) = self.finish_owned() {
                eprintln!("Shield child/group cleanup failed: {error}");
            }
            if let Err(error) = self.child.wait() {
                eprintln!("Shield child reap failed: {error}");
            }
        }
    }
}

#[cfg(test)]
mod tests;
