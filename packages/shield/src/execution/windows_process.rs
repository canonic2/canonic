//! Suspended native launches, independent kill-on-close ownership and bounded Job cleanup.
use super::Error;
use crate::policy;
use process_wrap::std::{ChildWrapper, CommandWrap, CommandWrapper, CreationFlags, JobObject};
use std::{
    io,
    os::windows::io::AsRawHandle,
    path::PathBuf,
    process::{Child, Command, Stdio},
    sync::Arc,
    time::{Duration, Instant},
};

pub(super) struct Signals(crate::windows_console::Signals);
impl Signals {
    pub(super) fn new() -> Result<Self, Error> {
        Ok(Self(crate::windows_console::Signals::new()?))
    }
    pub(super) fn interrupted(&self) -> bool {
        self.0.cancelled()
    }
    pub(super) fn exit_code(&self) -> Option<u8> {
        self.interrupted().then_some(130)
    }
}

#[derive(Debug)]
struct AttachJob(Arc<win32job::Job>);

fn io_error(error: impl std::fmt::Display) -> io::Error {
    io::Error::other(error.to_string())
}

impl CommandWrapper for AttachJob {
    fn post_spawn(
        &mut self,
        _: &mut Command,
        child: &mut Child,
        _: &CommandWrap,
    ) -> io::Result<()> {
        // JobObject's pre_spawn creates the child suspended. Attach our own
        // non-inheritable kill-on-close Job before its wrapper resumes threads.
        if let Err(error) = self.0.assign_process(child.as_raw_handle() as isize) {
            let _ = child.kill();
            let deadline = Instant::now() + Duration::from_millis(500);
            while child.try_wait()?.is_none() {
                if Instant::now() >= deadline {
                    return Err(io::Error::other("suspended child cleanup incomplete"));
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            return Err(io_error(error));
        }
        Ok(())
    }
}

pub(super) struct Outcome {
    pub(super) code: u8,
    pub(super) status: &'static str,
    pub(super) exit_code: Option<i32>,
    pub(super) signal: Option<i32>,
}

pub(super) struct OwnedChild {
    child: Box<dyn ChildWrapper>,
    job: Arc<win32job::Job>,
    finished: bool,
    console: ConsoleModes,
}

struct ConsoleModes {
    input: Option<u32>,
    output: Option<u32>,
    error: Option<u32>,
    restored: bool,
}
impl ConsoleModes {
    fn capture() -> io::Result<Self> {
        let mode = |result: io::Result<u32>| match result {
            Ok(mode) => Ok(Some(mode)),
            Err(error) if error.raw_os_error() == Some(6) => Ok(None),
            Err(error) => Err(error),
        };
        Ok(Self {
            input: mode(winapi_util::console::mode(io::stdin()))?,
            output: mode(winapi_util::console::mode(io::stdout()))?,
            error: mode(winapi_util::console::mode(io::stderr()))?,
            restored: false,
        })
    }
    fn restore(&mut self) -> io::Result<()> {
        if self.restored {
            return Ok(());
        }
        let mut failed = None;
        for result in [
            self.input
                .map(|mode| winapi_util::console::set_mode(io::stdin(), mode)),
            self.output
                .map(|mode| winapi_util::console::set_mode(io::stdout(), mode)),
            self.error
                .map(|mode| winapi_util::console::set_mode(io::stderr(), mode)),
        ]
        .into_iter()
        .flatten()
        {
            if let Err(error) = result {
                failed.get_or_insert(error);
            }
        }
        if let Some(error) = failed {
            return Err(error);
        }
        self.restored = true;
        Ok(())
    }
}
impl Drop for ConsoleModes {
    fn drop(&mut self) {
        if let Err(error) = self.restore() {
            eprintln!("Shield console restoration failed: {error}");
        }
    }
}

impl OwnedChild {
    pub(super) fn spawn(request: &policy::Request) -> Result<Self, Error> {
        // This private CLI consumer owns its process lifetime. Windows carries
        // this search policy into unpackaged child processes. Combined with a
        // cleared PATH, a missing OS DLL cannot fall back to the project cwd.
        let system = PathBuf::from(winsafe::GetSystemDirectory().map_err(io_error)?);
        winsafe::SetDllDirectory(Some(system.to_str().ok_or(Error::Unsupported)?))
            .map_err(io_error)?;
        let mut command = Command::new(&request.executable);
        command
            .env_clear()
            .envs(super::windows_binding::environment_for(request)?)
            .args(&request.arguments)
            .current_dir(&request.cwd)
            .stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit());
        Self::from_command(command)
    }

    fn from_command(command: Command) -> Result<Self, Error> {
        let console = ConsoleModes::capture()?;
        let mut limits = win32job::ExtendedLimitInfo::new();
        limits.limit_kill_on_job_close();
        let job = Arc::new(win32job::Job::create_with_limit_info(&limits).map_err(io_error)?);
        let mut command = CommandWrap::from(command);
        // A distinct console group prevents host Ctrl-C delivery from bypassing
        // the owner's cleanup. Cancellation terminates the owned Job explicitly.
        #[expect(
            clippy::default_trait_access,
            reason = "process-wrap exposes this transitive Windows type without re-exporting its name"
        )]
        let mut flags = CreationFlags(Default::default());
        flags.0.0 = 0x0000_0200; // CREATE_NEW_PROCESS_GROUP
        command
            .wrap(flags)
            .wrap(AttachJob(job.clone()))
            .wrap(JobObject);
        let child = command.spawn()?;
        Ok(Self {
            child,
            job,
            finished: false,
            console,
        })
    }

    pub(super) fn id(&self) -> u32 {
        self.child.id()
    }

    fn finish_owned(&mut self) -> Result<(), Error> {
        self.child.start_kill()?;
        let deadline = Instant::now() + Duration::from_millis(500);
        loop {
            // Do not use process-wrap's completion-port wait as a quiescence
            // proof: a Job notification need not mean ACTIVE_PROCESS_ZERO.
            if self
                .job
                .query_process_id_list()
                .map_err(io_error)?
                .is_empty()
            {
                return Ok(());
            }
            if Instant::now() >= deadline {
                return Err(Error::CleanupIncomplete);
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    fn finish(&mut self, status: &'static str, code: Option<u8>) -> Result<Outcome, Error> {
        self.finish_owned()?;
        // The only wrapper with child state is JobObject. Its direct inner
        // child is std::Child; our wrapper returned that native child unchanged.
        // The retained process handle reserves its PID even after try_wait.
        let result = self.child.inner_mut().wait()?;
        self.console.restore()?;
        self.finished = true;
        Ok(Outcome {
            code: code.unwrap_or_else(|| {
                result
                    .code()
                    .and_then(|code| u8::try_from(code).ok())
                    .unwrap_or(2)
            }),
            status,
            exit_code: result.code(),
            signal: None,
        })
    }

    pub(super) fn abort(&mut self) -> Result<Outcome, Error> {
        self.finish("aborted-after-launch", Some(2))
    }

    pub(super) fn wait(
        &mut self,
        signals: &Signals,
        started: Instant,
        timeout: u64,
    ) -> Result<Outcome, Error> {
        loop {
            if let Some(status) = self.child.inner_mut().try_wait()? {
                return self.finish(
                    if status.success() {
                        "completed"
                    } else {
                        "failed"
                    },
                    None,
                );
            }
            if let Some(code) = signals.exit_code() {
                return self.finish("cancelled", Some(code));
            }
            if started.elapsed().as_millis() >= u128::from(timeout) {
                return self.finish("timed-out", Some(124));
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

impl Drop for OwnedChild {
    fn drop(&mut self) {
        if !self.finished {
            match self.finish_owned() {
                Ok(()) => {
                    let _ = self.child.inner_mut().wait();
                }
                Err(error) => eprintln!("Shield child/Job cleanup failed: {error}"),
            }
        }
    }
}

#[cfg(test)]
mod tests;
