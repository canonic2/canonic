//! Owns one interactive terminal lease and restores its modes on drop or unwinding.

mod inline;
pub use inline::{Capabilities, InlineOptions, InlineSession, PausedInline};

#[cfg(unix)]
use crossterm::event::{
    KeyboardEnhancementFlags, PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
};
use crossterm::{
    cursor::{Hide, Show},
    event::{self, DisableBracketedPaste, EnableBracketedPaste, Event},
    execute,
    terminal::{self, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{Frame, Terminal, backend::CrosstermBackend};
use std::{
    io::{self, IsTerminal, Stdout},
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};

static ACTIVE: AtomicBool = AtomicBool::new(false);

/// The sole owner of raw mode, alternate screen and bracketed paste in this process.
pub struct TerminalSession {
    terminal: Option<Terminal<CrosstermBackend<Stdout>>>,
    raw: bool,
    screen: bool,
    paste: bool,
    held: Option<()>,
    #[cfg(unix)]
    keyboard: Option<KeyboardEnhancementFlags>,
    #[cfg(unix)]
    original: Option<rustix::termios::Termios>,
}

impl TerminalSession {
    /// Release terminal modes, run a foreground host operation, then reacquire them.
    /// Both results are returned so an operation error cannot hide a restoration error.
    /// Terminal modes are restored before the callback, including on callback unwind.
    pub fn handoff<T>(
        self,
        operation: impl FnOnce() -> io::Result<T>,
    ) -> (io::Result<Self>, io::Result<T>) {
        #[cfg(unix)]
        let original = self.original.clone();
        if let Err(error) = self.close() {
            return (
                Err(error),
                Err(io::Error::other(
                    "Terminal release failed; operation was not started",
                )),
            );
        }
        let result = operation();
        let terminal = Self::open();
        #[cfg(unix)]
        let terminal = terminal.map(|mut terminal| {
            terminal.original = original;
            terminal
        });
        (terminal, result)
    }
    /// Acquire an interactive terminal without changing noninteractive output.
    ///
    /// # Errors
    /// Returns an error for piped input/output, an existing lease, or terminal I/O failure.
    pub fn open() -> io::Result<Self> {
        let mut session = Self::acquire_modes(true)?;
        session.terminal = Some(Terminal::new(CrosstermBackend::new(io::stdout()))?);
        Ok(session)
    }

    fn acquire_modes(screen: bool) -> io::Result<Self> {
        if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
            return Err(io::Error::other(
                "The gallery requires an interactive terminal; use --list for plain output",
            ));
        }
        if ACTIVE
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return Err(io::Error::other("A terminal session is already active"));
        }
        let mut session = Self {
            terminal: None,
            raw: false,
            screen: false,
            paste: false,
            held: Some(()),
            #[cfg(unix)]
            keyboard: None,
            #[cfg(unix)]
            original: None,
        };
        #[cfg(unix)]
        {
            session.original = Some(rustix::termios::tcgetattr(io::stdin())?);
        }
        terminal::enable_raw_mode()?;
        session.raw = true;
        session.screen = screen;
        if screen {
            execute!(io::stdout(), EnterAlternateScreen, Hide)?;
        } else {
            execute!(io::stdout(), Hide)?;
        }
        session.paste = true;
        execute!(io::stdout(), EnableBracketedPaste)?;
        #[cfg(unix)]
        {
            let flags = KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES;
            session.keyboard = Some(flags);
            execute!(io::stdout(), PushKeyboardEnhancementFlags(flags))?;
        }
        Ok(session)
    }

    /// Draw using the caller's application state.
    ///
    /// # Errors
    /// Returns terminal rendering errors.
    pub fn draw(&mut self, render: impl FnOnce(&mut Frame<'_>)) -> io::Result<()> {
        self.terminal
            .as_mut()
            .ok_or_else(|| io::Error::other("Terminal is closed"))?
            .draw(render)?;
        Ok(())
    }

    /// Poll input, permitting the host to advance independent activity between events.
    ///
    /// # Errors
    /// Returns terminal input errors.
    pub fn poll(&self, timeout: Duration) -> io::Result<Option<Event>> {
        if self.terminal.is_none() {
            return Err(io::Error::other("Terminal is closed"));
        }
        if event::poll(timeout)? {
            event::read().map(Some)
        } else {
            Ok(None)
        }
    }
    /// Explicitly restore the lease, reporting mode and output errors.
    /// # Errors
    /// Returns all observed terminal restoration failures; drop remains a fallback.
    pub fn close(mut self) -> io::Result<()> {
        self.restore()
    }
    fn restore(&mut self) -> io::Result<()> {
        self.terminal = None;
        let mut errors = Vec::new();
        let mut record = |result: io::Result<()>| {
            if let Err(error) = result {
                errors.push(error.to_string());
            }
        };
        #[cfg(unix)]
        if self.keyboard.take().is_some() {
            record(execute!(io::stdout(), PopKeyboardEnhancementFlags));
        }
        if std::mem::take(&mut self.paste) {
            record(execute!(io::stdout(), DisableBracketedPaste));
        }
        if std::mem::take(&mut self.screen) {
            record(execute!(io::stdout(), Show, LeaveAlternateScreen));
        } else if self.held.is_some() {
            record(execute!(io::stdout(), Show));
        }
        if std::mem::take(&mut self.raw) {
            record(terminal::disable_raw_mode());
        }
        #[cfg(unix)]
        if let Some(original) = self.original.take() {
            record(
                rustix::termios::tcsetattr(
                    io::stdin(),
                    rustix::termios::OptionalActions::Now,
                    &original,
                )
                .map_err(Into::into),
            );
        }
        if self.held.take().is_some() {
            ACTIVE.store(false, Ordering::Release);
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(io::Error::other(errors.join("; ")))
        }
    }
}

impl Drop for TerminalSession {
    fn drop(&mut self) {
        let _ = self.restore();
    }
}
