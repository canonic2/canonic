//! Owns one interactive terminal lease and restores its modes on drop or unwinding.

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
    #[cfg(unix)]
    keyboard: Option<KeyboardEnhancementFlags>,
}

impl TerminalSession {
    /// Acquire an interactive terminal without changing noninteractive output.
    ///
    /// # Errors
    /// Returns an error for piped input/output, an existing lease, or terminal I/O failure.
    pub fn open() -> io::Result<Self> {
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
            #[cfg(unix)]
            keyboard: None,
        };
        terminal::enable_raw_mode()?;
        session.raw = true;
        session.screen = true;
        execute!(io::stdout(), EnterAlternateScreen, Hide)?;
        session.paste = true;
        execute!(io::stdout(), EnableBracketedPaste)?;
        #[cfg(unix)]
        {
            let flags = KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES;
            session.keyboard = Some(flags);
            execute!(io::stdout(), PushKeyboardEnhancementFlags(flags))?;
        }
        session.terminal = Some(Terminal::new(CrosstermBackend::new(io::stdout()))?);
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
}

impl Drop for TerminalSession {
    fn drop(&mut self) {
        #[cfg(unix)]
        if self.keyboard.is_some() {
            let _ = execute!(io::stdout(), PopKeyboardEnhancementFlags);
        }
        if self.paste {
            let _ = execute!(io::stdout(), DisableBracketedPaste);
        }
        if self.screen {
            let _ = execute!(io::stdout(), Show, LeaveAlternateScreen);
        }
        if self.raw {
            let _ = terminal::disable_raw_mode();
        }
        ACTIVE.store(false, Ordering::Release);
    }
}
