//! Owns gallery-only suspension requests, process stops and foreground return.

use crate::{
    event::{Event, KeyCode, KeyEventKind, KeyModifiers},
    runtime::{InlineSession, TerminalSession},
};
use std::io;
#[cfg(unix)]
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

pub(super) fn key(event: &Event) -> bool {
    matches!(event, Event::Key(k) if k.kind == KeyEventKind::Press
        && k.code == KeyCode::Char('z')
        && k.modifiers == (KeyModifiers::CONTROL | KeyModifiers::ALT))
}

/// Scoped flag registration. Signal delivery does no terminal I/O or locking.
pub(super) struct Suspension {
    #[cfg(unix)]
    pending: Arc<AtomicBool>,
    #[cfg(unix)]
    registration: Option<signal_hook::SigId>,
}
impl Suspension {
    pub(super) fn install() -> io::Result<Self> {
        #[cfg(unix)]
        {
            let pending = Arc::new(AtomicBool::new(false));
            let registration = if io::IsTerminal::is_terminal(&io::stdin())
                && io::IsTerminal::is_terminal(&io::stdout())
                && available().is_ok()
            {
                Some(signal_hook::flag::register(
                    signal_hook::consts::SIGTSTP,
                    pending.clone(),
                )?)
            } else {
                None
            };
            Ok(Self {
                pending,
                registration,
            })
        }
        #[cfg(not(unix))]
        {
            Ok(Self {})
        }
    }
    pub(super) fn requested(&self) -> bool {
        #[cfg(unix)]
        {
            self.pending.swap(false, Ordering::AcqRel)
        }
        #[cfg(not(unix))]
        {
            false
        }
    }
}
impl Drop for Suspension {
    fn drop(&mut self) {
        #[cfg(unix)]
        if let Some(id) = self.registration.take() {
            signal_hook::low_level::unregister(id);
        }
    }
}

#[cfg(unix)]
pub(super) fn available() -> io::Result<()> {
    if rustix::termios::tcgetpgrp(io::stdin())? != rustix::process::getpgrp() {
        return Err(io::Error::other(
            "gallery is not the terminal foreground job",
        ));
    }
    Ok(())
}
#[cfg(not(unix))]
pub(super) fn available() -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "job suspension needs a Unix controlling terminal",
    ))
}

#[cfg(unix)]
fn stop() -> io::Result<()> {
    use rustix::process::{Signal, getpid, kill_process};
    loop {
        // Stop only this host, after its lease has released all terminal modes.
        kill_process(getpid(), Signal::STOP)?;
        if rustix::termios::tcgetpgrp(io::stdin())? == rustix::process::getpgrp() {
            return Ok(());
        }
        // Background continuation never reacquires modes or redraws.
    }
}
#[cfg(not(unix))]
fn stop() -> io::Result<()> {
    available()
}

pub(super) fn inline(session: InlineSession) -> io::Result<InlineSession> {
    let alternate = session.alternate();
    let paused = session.pause()?;
    stop()?;
    let mut resumed = paused.resume()?;
    if alternate {
        resumed.enter_alternate()?;
    }
    Ok(resumed)
}
pub(super) fn full(session: TerminalSession) -> io::Result<TerminalSession> {
    let (resumed, stopped) = session.handoff(stop);
    match (resumed, stopped) {
        (Ok(session), Ok(())) => Ok(session),
        (Err(resume), Err(stop)) => Err(io::Error::other(format!(
            "suspension: {stop}; resume: {resume}"
        ))),
        (Err(error), _) | (_, Err(error)) => Err(error),
    }
}
