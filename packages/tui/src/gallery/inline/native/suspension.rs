//! Owns opt-in gallery job control; reusable terminal leases never send signals.

use crate::runtime::InlineSession;
use std::io;

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
pub(super) fn run(session: InlineSession) -> io::Result<InlineSession> {
    use rustix::process::{Signal, getpid, kill_process};
    let alternate = session.alternate();
    let paused = session.pause()?;
    // Stop only this host, never the shell or an unrelated process group. SIGSTOP
    // guarantees suspension even if an inherited SIGTSTP disposition is ignored.
    loop {
        kill_process(getpid(), Signal::STOP)?;
        if rustix::termios::tcgetpgrp(io::stdin())? == rustix::process::getpgrp() {
            break;
        }
        // A background SIGCONT must not acquire modes or draw on the shell's tty.
    }
    let mut resumed = paused.resume()?;
    if alternate {
        resumed.enter_alternate()?;
    }
    Ok(resumed)
}
#[cfg(not(unix))]
pub(super) fn run(_session: InlineSession) -> io::Result<InlineSession> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "job suspension unavailable",
    ))
}
