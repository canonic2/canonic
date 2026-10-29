//! Native signal ownership while the operator holds an inline terminal lease.
//!
//! Handlers are registered once, at the first prompt, and kept for the rest of
//! the process: signal-hook cannot restore the previous disposition when they
//! are removed. While a prompt is active, INT/TERM/HUP cancel it and TSTP
//! releases modes before stopping. Otherwise each signal runs its default
//! action, so the process can still be interrupted, terminated and suspended.
use super::{Error, InlineSession};
#[cfg(unix)]
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};

#[cfg(unix)]
struct Registration {
    /// True outside prompts: signals then run their default action.
    idle: Arc<AtomicBool>,
    cancel: Arc<AtomicBool>,
    suspend: Arc<AtomicBool>,
}

#[cfg(unix)]
static REGISTRATION: Mutex<Option<Arc<Registration>>> = Mutex::new(None);

#[cfg(unix)]
fn registration() -> Result<Arc<Registration>, Error> {
    let mut slot = REGISTRATION
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(registration) = slot.as_ref() {
        return Ok(registration.clone());
    }
    let registration = Arc::new(Registration {
        idle: Arc::new(AtomicBool::new(true)),
        cancel: Arc::new(AtomicBool::new(false)),
        suspend: Arc::new(AtomicBool::new(false)),
    });
    for (signal, flag) in [
        (signal_hook::consts::SIGINT, &registration.cancel),
        (signal_hook::consts::SIGTERM, &registration.cancel),
        (signal_hook::consts::SIGHUP, &registration.cancel),
        (signal_hook::consts::SIGTSTP, &registration.suspend),
    ] {
        signal_hook::flag::register(signal, flag.clone())?;
        signal_hook::flag::register_conditional_default(signal, registration.idle.clone())?;
    }
    *slot = Some(registration.clone());
    Ok(registration)
}

/// One prompt's claim on the process-wide handlers.
pub(super) struct Signals {
    #[cfg(unix)]
    registration: Arc<Registration>,
}
impl Signals {
    pub(super) fn new() -> Result<Self, Error> {
        #[cfg(unix)]
        {
            let registration = registration()?;
            // Flags raised while idle, such as a stop and continue, are not this prompt's.
            registration.cancel.store(false, Ordering::SeqCst);
            registration.suspend.store(false, Ordering::SeqCst);
            registration.idle.store(false, Ordering::SeqCst);
            Ok(Self { registration })
        }
        #[cfg(not(unix))]
        {
            Ok(Self {})
        }
    }
    pub(super) fn check(&self, session: InlineSession) -> Result<InlineSession, Error> {
        #[cfg(unix)]
        {
            if self.registration.cancel.load(Ordering::SeqCst) {
                session.finish("Shield → cancelled")?;
                return Err(Error::Cancelled);
            }
            if self.registration.suspend.swap(false, Ordering::SeqCst) {
                let paused = session.pause()?;
                loop {
                    rustix::process::kill_process(
                        rustix::process::getpid(),
                        rustix::process::Signal::STOP,
                    )
                    .map_err(std::io::Error::from)?;
                    if self.cancelled() {
                        return Err(Error::Cancelled);
                    }
                    if rustix::termios::tcgetpgrp(std::io::stdin()).map_err(std::io::Error::from)?
                        == rustix::process::getpgrp()
                    {
                        break;
                    }
                    // Background continuation never reacquires modes or redraws.
                }
                return Ok(paused.resume()?);
            }
        }
        Ok(session)
    }
    pub(super) fn cancelled(&self) -> bool {
        #[cfg(unix)]
        {
            self.registration.cancel.load(Ordering::SeqCst)
        }
        #[cfg(not(unix))]
        {
            false
        }
    }
}
impl Drop for Signals {
    fn drop(&mut self) {
        #[cfg(unix)]
        self.registration.idle.store(true, Ordering::SeqCst);
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::{
        os::unix::process::ExitStatusExt,
        process::{Command, Stdio},
        time::Duration,
    };

    /// Child: claims the handlers as one prompt would, then raises a signal.
    #[test]
    #[ignore = "runs as a child of signals_take_their_default_action_once_a_prompt_ends"]
    fn signal_child_process() {
        let signal = std::env::var("SHIELD_TEST_SIGNAL")
            .unwrap()
            .parse()
            .unwrap();
        let guard = Signals::new().unwrap();
        if std::env::var_os("SHIELD_TEST_HOLD").is_some() {
            signal_hook::low_level::raise(signal).unwrap();
            assert!(guard.cancelled(), "an active prompt cancels instead");
            return;
        }
        drop(guard);
        signal_hook::low_level::raise(signal).unwrap();
        // Reaching the end means the signal was ignored after the prompt.
        std::thread::sleep(Duration::from_secs(5));
    }

    fn child(signal: i32, hold: bool) -> std::process::ExitStatus {
        let mut command = Command::new(std::env::current_exe().expect("test executable path"));
        command
            .args([
                "--exact",
                "review::operator::terminal::inline::signals::tests::signal_child_process",
                "--ignored",
                "--nocapture",
            ])
            .env("SHIELD_TEST_SIGNAL", signal.to_string())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        if hold {
            command.env("SHIELD_TEST_HOLD", "1");
        }
        command.status().expect("signal test child runs")
    }

    #[test]
    fn signals_take_their_default_action_once_a_prompt_ends() {
        for signal in [
            signal_hook::consts::SIGINT,
            signal_hook::consts::SIGTERM,
            signal_hook::consts::SIGHUP,
        ] {
            assert_eq!(child(signal, false).signal(), Some(signal), "{signal}");
        }
        assert!(child(signal_hook::consts::SIGTERM, true).success());
    }
}
