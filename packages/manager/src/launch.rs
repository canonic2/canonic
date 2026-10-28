//! Detached re-execution and already-registered service activation.

use crate::{Error, Result};
use std::{
    ffi::OsString,
    fs::File,
    path::PathBuf,
    process::{Command, Stdio},
};

/// The client can start through either detached spawn or an OS registration.
pub trait Launcher {
    /// Request startup; readiness is observed separately over the private endpoint.
    ///
    /// # Errors
    /// Returns activation or process-start errors, never an application result.
    fn start(&self) -> Result<()>;
}

/// Re-execute the current binary with a hidden product subcommand.
pub struct Spawn {
    /// Hidden subcommand and arguments, including the explicit root when selected.
    pub args: Vec<OsString>,
    /// Complete environment; all inherited variables are cleared first.
    pub environment: Vec<(OsString, OsString)>,
    /// Product-owned log already opened with its chosen privacy policy.
    pub log: File,
    /// Explicit working directory, independent of the client's working directory.
    pub directory: PathBuf,
}

impl Spawn {
    /// Spawn and arrange kernel-wait reaping in a client thread.
    ///
    /// # Errors
    /// Returns unsupported, executable lookup, descriptor duplication or spawn errors.
    pub fn start_reaped(
        &self,
    ) -> Result<std::thread::JoinHandle<std::io::Result<std::process::ExitStatus>>> {
        crate::supported()?;
        let mut command = Command::new(std::env::current_exe()?);
        command
            .args(&self.args)
            .env_clear()
            .envs(self.environment.iter().cloned())
            .current_dir(&self.directory)
            .stdin(Stdio::null())
            .stdout(self.log.try_clone()?)
            .stderr(self.log.try_clone()?);
        detach(&mut command);
        let mut child = command.spawn()?;
        Ok(std::thread::spawn(move || child.wait()))
    }
}
impl Launcher for Spawn {
    fn start(&self) -> Result<()> {
        self.start_reaped()?;
        Ok(())
    }
}

#[cfg(unix)]
#[allow(
    unsafe_code,
    reason = "std exposes no safe pre-exec setsid hook; the child calls only the async-signal-safe setsid syscall"
)]
fn detach(command: &mut Command) {
    use std::os::unix::process::CommandExt;
    // SAFETY: the callback captures no state, allocates nothing and takes no locks.
    // setsid creates both a new session and a new process group before exec.
    unsafe {
        command.pre_exec(|| {
            nix::unistd::setsid()
                .map(|_| ())
                .map_err(std::io::Error::from)
        });
    }
}
#[cfg(not(unix))]
fn detach(_: &mut Command) {}

/// Activate an existing service registration. Registration and removal are product work.
pub enum Service {
    /// Full launchd service target, e.g. `gui/501/com.example.acme`.
    Launchd(String),
    /// A systemd user unit name, e.g. `acme-manager.service`.
    Systemd(String),
}
impl Launcher for Service {
    fn start(&self) -> Result<()> {
        let mut command = match self {
            Self::Launchd(target) if cfg!(target_os = "macos") => {
                let mut c = Command::new("/bin/launchctl");
                c.args(["kickstart", target]);
                c
            }
            Self::Systemd(unit) if cfg!(target_os = "linux") => {
                if unit.starts_with('-') {
                    return Err(Error::Options);
                }
                let mut c = Command::new("/usr/bin/systemctl");
                c.args(["--user", "start", "--", unit]);
                c
            }
            _ => return Err(Error::Unsupported),
        };
        // Activation doesn't execute the manager as a client child. The registration
        // supplies its environment; no caller variables are forwarded to the service.
        let status = command
            .env_clear()
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()?;
        if status.success() {
            Ok(())
        } else {
            Err(Error::Io(std::io::Error::other(
                "service activation failed",
            )))
        }
    }
}
