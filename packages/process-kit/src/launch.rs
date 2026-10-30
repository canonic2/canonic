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
        let mut child = spawn_detached(&mut command)?;
        Ok(std::thread::spawn(move || child.wait()))
    }
}

#[cfg(not(windows))]
fn spawn_detached(command: &mut Command) -> std::io::Result<std::process::Child> {
    detach(command);
    command.spawn()
}

/// A detached process with no console, in its own process group, outside the
/// client's job when the job allows breaking away; closing the client's
/// console or job then does not end it.
#[cfg(windows)]
fn spawn_detached(command: &mut Command) -> std::io::Result<std::process::Child> {
    use std::os::windows::process::CommandExt;
    const DETACHED_PROCESS: u32 = 0x0000_0008;
    const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
    const CREATE_BREAKAWAY_FROM_JOB: u32 = 0x0100_0000;
    command.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP | CREATE_BREAKAWAY_FROM_JOB);
    match command.spawn() {
        Ok(child) => Ok(child),
        // A job that forbids breakaway refuses the flag; start inside it instead.
        Err(error) if error.raw_os_error() == Some(5) => {
            command.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP);
            command.spawn()
        }
        Err(error) => Err(error),
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
#[cfg(not(any(unix, windows)))]
fn detach(_: &mut Command) {}

/// Activate an existing service registration. Registration and removal are product work.
pub enum Service {
    /// Full launchd service target, e.g. `gui/501/com.example.acme`.
    Launchd(String),
    /// A systemd user unit name, e.g. `acme-manager.service`.
    Systemd(String),
    /// A Windows Task Scheduler task name, e.g. `Acme\Manager`.
    ScheduledTask(String),
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
            Self::ScheduledTask(task) if cfg!(windows) => {
                scheduled_task_command(task, std::env::var_os("SystemRoot"))?
            }
            _ => return Err(Error::Unsupported),
        };
        // Activation doesn't execute the manager as a client child. The registration
        // supplies its environment; no caller variables are forwarded to the service.
        command.env_clear();
        if cfg!(windows) {
            // schtasks itself needs the system directories.
            for name in ["SystemRoot", "windir"] {
                if let Some(value) = std::env::var_os(name) {
                    command.env(name, value);
                }
            }
        }
        let status = command
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

/// Build without launching so validation never activates a real registration.
fn scheduled_task_command(task: &str, system_root: Option<OsString>) -> Result<Command> {
    if task.starts_with(['-', '/']) {
        return Err(Error::Options);
    }
    let root = PathBuf::from(system_root.ok_or(Error::Options)?);
    if !root.is_absolute() {
        return Err(Error::Options);
    }
    let mut command = Command::new(root.join("System32").join("schtasks.exe"));
    command.args(["/Run", "/TN", task]);
    Ok(command)
}

#[cfg(test)]
mod tests {
    //! Validate scheduler executable selection and arguments without activating tasks.

    use super::*;

    #[test]
    fn scheduled_task_uses_an_absolute_system_executable_and_one_task_argument() {
        let root = std::env::current_dir().unwrap();
        let task = r"\Acme\Manager with spaces";
        let command = scheduled_task_command(task, Some(root.clone().into_os_string())).unwrap();
        assert_eq!(
            command.get_program(),
            root.join("System32").join("schtasks.exe")
        );
        assert_eq!(
            command.get_args().map(OsString::from).collect::<Vec<_>>(),
            ["/Run", "/TN", task].map(OsString::from)
        );
    }

    #[test]
    fn scheduled_task_refuses_missing_empty_or_relative_system_root() {
        for root in [None, Some(OsString::new()), Some("Windows".into())] {
            assert!(matches!(
                scheduled_task_command(r"Acme\Manager", root),
                Err(Error::Options)
            ));
        }
    }

    #[test]
    fn scheduled_task_refuses_option_like_task_names() {
        let root = std::env::current_dir().unwrap().into_os_string();
        for task in ["-Run", "/Run"] {
            assert!(matches!(
                scheduled_task_command(task, Some(root.clone())),
                Err(Error::Options)
            ));
        }
    }
}
