//! Ctrl-Z suspension for prompt hosts. Prompts register no signal handlers, so
//! the host's signal dispositions are unchanged before, during and after them.
use super::InlineSession;
use crate::event::{Event, KeyCode, KeyEventKind, KeyModifiers};
use std::io;

/// Raw mode delivers job-control keys as events instead of SIGTSTP.
/// Ctrl-Alt-Z always suspends, as in the gallery; plain Ctrl-Z suspends only
/// where it is not undo, which the caller states with `plain`.
pub(super) fn requested(event: &Event, plain: bool) -> bool {
    matches!(event, Event::Key(key) if key.kind == KeyEventKind::Press
        && key.modifiers.contains(KeyModifiers::CONTROL)
        && key.code == KeyCode::Char('z')
        && (plain || key.modifiers.contains(KeyModifiers::ALT)))
}

/// Release the terminal, stop the process as job control would, and resume
/// below any shell output once the process is in the foreground again.
/// Platforms without job control keep the prompt open.
pub(super) fn suspend(session: &mut Option<InlineSession>) -> io::Result<()> {
    #[cfg(unix)]
    {
        let paused = session
            .take()
            .ok_or_else(|| io::Error::other("Terminal is closed"))?
            .pause()?;
        loop {
            rustix::process::kill_process(
                rustix::process::getpid(),
                rustix::process::Signal::STOP,
            )?;
            if rustix::termios::tcgetpgrp(io::stdin())? == rustix::process::getpgrp() {
                break;
            }
        }
        *session = Some(paused.resume()?);
    }
    #[cfg(not(unix))]
    let _ = session;
    Ok(())
}
