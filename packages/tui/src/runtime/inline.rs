//! Owns fresh inline rows and temporary alternate-screen borrowing under one terminal lease.

use super::TerminalSession;
use crate::text::wrapped;
use crossterm::{
    cursor::{self, Hide, MoveTo},
    event::{self, Event},
    execute, queue,
    style::{Print, ResetColor},
    terminal::{self, Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    Frame, Terminal, TerminalOptions, Viewport, backend::CrosstermBackend, layout::Rect,
};
use std::{
    collections::VecDeque,
    io::{self, Write},
    time::Duration,
};

/// Host-resolved capabilities. Unknown cursor control requires plain fallback.
#[derive(Clone, Copy, Debug, Default)]
pub struct Capabilities {
    pub cursor: bool,
    pub alternate: bool,
}
/// Explicit inline acquisition; constructors perform no terminal work.
#[derive(Clone, Copy, Debug)]
pub struct InlineOptions {
    pub height: u16,
    pub capabilities: Capabilities,
}
impl Default for InlineOptions {
    fn default() -> Self {
        Self {
            height: 8,
            capabilities: Capabilities::default(),
        }
    }
}

/// One bounded live region. Printed output and caller application state remain host-owned.
pub struct InlineSession {
    lease: Option<TerminalSession>,
    terminal: Option<Terminal<CrosstermBackend<io::Stdout>>>,
    options: InlineOptions,
    size: (u16, u16),
    area: Rect,
    alternate: bool,
    anchor_lost: bool,
    deferred: VecDeque<String>,
    deferred_bytes: usize,
    failed: bool,
}
/// Released terminal ownership; the host keeps its editor/data and owns job-control signals.
pub struct PausedInline {
    options: InlineOptions,
    #[cfg(unix)]
    original: Option<rustix::termios::Termios>,
}
impl PausedInline {
    /// Resume below intervening shell output using a newly verified anchor.
    /// # Errors
    /// Returns acquisition/capability errors without consuming caller data.
    pub fn resume(self) -> io::Result<InlineSession> {
        let mut session = InlineSession::open(self.options)?;
        #[cfg(unix)]
        if let Some(lease) = &mut session.lease {
            lease.original = self.original;
        }
        Ok(session)
    }
}
impl InlineSession {
    /// Acquire raw/paste modes, verify the cursor and reserve fresh rows below it.
    /// # Errors
    /// Unknown capabilities, noninteractive/tiny terminals and failed cursor reports return
    /// errors. Hosts use numbered text fallback; no anchor is guessed.
    pub fn open(options: InlineOptions) -> io::Result<Self> {
        if !options.capabilities.cursor {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "no safe cursor control; use plain input",
            ));
        }
        if options.height == 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "inline height must be positive",
            ));
        }
        if !io::IsTerminal::is_terminal(&io::stdin()) || !io::IsTerminal::is_terminal(&io::stdout())
        {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "inline input requires an interactive terminal",
            ));
        }
        let size = terminal::size()?;
        if size.0 < 20 || size.1 < 5 {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "window too small; use plain input",
            ));
        }
        let lease = TerminalSession::acquire_modes(false)?;
        let (_, row) = cursor::position()?;
        if row >= size.1 {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "cursor report outside window; use plain input",
            ));
        }
        let mut session = Self {
            lease: Some(lease),
            terminal: None,
            options,
            size,
            area: Rect::default(),
            alternate: false,
            anchor_lost: false,
            deferred: VecDeque::new(),
            deferred_bytes: 0,
            failed: false,
        };
        session.reserve(row)?;
        Ok(session)
    }
    #[must_use]
    pub fn area(&self) -> Rect {
        self.area
    }
    #[must_use]
    pub fn alternate(&self) -> bool {
        self.alternate
    }
    /// Report a resize from an external event source; stale rows will be abandoned.
    pub fn invalidate_anchor(&mut self) {
        self.anchor_lost = true;
    }
    /// Require enough visible rows before dispatching a confirmation to a component.
    #[must_use]
    pub fn confirmation_available(&self) -> bool {
        terminal::size().is_ok_and(|(width, height)| {
            width >= 20 && height >= 5 && (self.alternate || self.area.height >= 3)
        })
    }
    /// Invalidate the live buffer for explicit redraw without clearing printed history.
    /// # Errors
    /// Returns terminal errors and refuses an unavailable lease.
    pub fn redraw(&mut self) -> io::Result<()> {
        self.check()?;
        if self.alternate {
            self.terminal = Some(Terminal::new(CrosstermBackend::new(io::stdout()))?);
        } else if terminal::size()? != self.size {
            self.fresh_bottom()?;
        } else {
            self.clear_owned()?;
            self.terminal = Some(Terminal::with_options(
                CrosstermBackend::new(io::stdout()),
                TerminalOptions {
                    viewport: Viewport::Fixed(self.area),
                },
            )?);
        }
        Ok(())
    }
    fn reserve(&mut self, row: u16) -> io::Result<()> {
        let height = self
            .options
            .height
            .min(self.size.1.saturating_sub(2))
            .max(1);
        let mut out = io::stdout();
        for _ in 0..height {
            queue!(out, Print("\r\n"))?;
        }
        out.flush()?;
        let bottom = row
            .saturating_add(height)
            .min(self.size.1.saturating_sub(1));
        self.area = Rect::new(0, bottom.saturating_sub(height - 1), self.size.0, height);
        self.terminal = Some(Terminal::with_options(
            CrosstermBackend::new(out),
            TerminalOptions {
                viewport: Viewport::Fixed(self.area),
            },
        )?);
        Ok(())
    }
    fn fresh_bottom(&mut self) -> io::Result<()> {
        self.terminal = None;
        self.size = terminal::size()?;
        execute!(io::stdout(), MoveTo(0, self.size.1.saturating_sub(1)), Hide)?;
        self.reserve(self.size.1.saturating_sub(1))?;
        self.anchor_lost = false;
        Ok(())
    }
    fn check(&self) -> io::Result<()> {
        if self.failed || self.lease.is_none() {
            Err(io::Error::other("inline terminal is unavailable"))
        } else {
            Ok(())
        }
    }
    /// Redraw only owned rows. Resize reserves a new region below reflowed content.
    /// # Errors
    /// Returns terminal errors; failed output stops further drawing until released.
    pub fn draw(&mut self, render: impl FnOnce(&mut Frame<'_>)) -> io::Result<()> {
        self.check()?;
        let result = self.draw_inner(render);
        if result.is_err() {
            self.failed = true;
        }
        result
    }
    fn draw_inner(&mut self, render: impl FnOnce(&mut Frame<'_>)) -> io::Result<()> {
        if terminal::size()? != self.size {
            self.anchor_lost = true;
        }
        if !self.alternate && self.anchor_lost {
            self.fresh_bottom()?;
        }
        self.terminal
            .as_mut()
            .ok_or_else(|| io::Error::other("inline terminal is closed"))?
            .draw(render)?;
        Ok(())
    }
    /// Poll under the existing lease; components never read input themselves.
    /// # Errors
    /// Returns input errors or refuses a stopped session.
    pub fn poll(&mut self, timeout: Duration) -> io::Result<Option<Event>> {
        self.check()?;
        if event::poll(timeout)? {
            let event = event::read()?;
            if matches!(event, Event::Resize(..)) {
                self.invalidate_anchor();
            }
            Ok(Some(event))
        } else {
            Ok(None)
        }
    }
    fn clear_owned(&mut self) -> io::Result<()> {
        // Never reuse stale coordinates after the terminal has reflowed its contents.
        if self.anchor_lost || terminal::size()? != self.size {
            return self.fresh_bottom();
        }
        self.terminal = None;
        let mut out = io::stdout();
        for y in self.area.y..self.area.bottom() {
            queue!(out, MoveTo(0, y), Clear(ClearType::CurrentLine))?;
        }
        queue!(out, MoveTo(0, self.area.y), ResetColor)?;
        out.flush()
    }
    /// Print settled host text above the region, or defer it while a borrowed screen is up.
    /// # Errors
    /// Bounded output overflow or I/O failure is reported without implicit retries.
    pub fn print_before(&mut self, text: &str) -> io::Result<()> {
        self.check()?;
        if text.len() > 65536 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "printed record exceeds 64 KiB",
            ));
        }
        if self.alternate {
            if self.deferred.len() >= 128
                || self.deferred_bytes.saturating_add(text.len()) > 1024 * 1024
            {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "deferred output limit reached",
                ));
            }
            self.deferred.push_back(text.into());
            self.deferred_bytes += text.len();
            return Ok(());
        }
        let result = self.print_inner(text);
        if result.is_err() {
            self.failed = true;
        }
        result
    }
    fn print_inner(&mut self, text: &str) -> io::Result<()> {
        self.clear_owned()?;
        let mut out = io::stdout();
        let lines = wrapped(&printable(text), self.size.0);
        for line in &lines {
            queue!(out, Print(line), Print("\r\n"))?;
        }
        out.flush()?;
        let row = self
            .area
            .y
            .saturating_add(u16::try_from(lines.len()).unwrap_or(u16::MAX))
            .min(self.size.1.saturating_sub(1));
        self.reserve(row)
    }
    /// Borrow the alternate screen with the same lease; caller views/data stay alive.
    /// # Errors
    /// Unsupported/nested borrowing and output failures leave no affirmative result.
    pub fn enter_alternate(&mut self) -> io::Result<()> {
        self.check()?;
        if self.alternate {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "alternate screen already borrowed",
            ));
        }
        if !self.options.capabilities.alternate {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "view needs alternate screen",
            ));
        }
        self.terminal = None;
        if let Some(lease) = &mut self.lease {
            lease.screen = true;
        }
        self.alternate = true;
        let result = execute!(io::stdout(), EnterAlternateScreen, Hide).and_then(|()| {
            self.terminal = Some(Terminal::new(CrosstermBackend::new(io::stdout()))?);
            Ok(())
        });
        if result.is_err() {
            self.failed = true;
        }
        result
    }
    /// Return to the normal screen and flush deferred output in host order.
    /// # Errors
    /// Returns restoration/write errors; partially written records are never retried automatically.
    pub fn leave_alternate(&mut self) -> io::Result<()> {
        self.check()?;
        if !self.alternate {
            return Ok(());
        }
        self.terminal = None;
        execute!(io::stdout(), LeaveAlternateScreen, Hide)?;
        if let Some(lease) = &mut self.lease {
            lease.screen = false;
        }
        self.alternate = false;
        if !self.anchor_lost && terminal::size()? == self.size {
            self.terminal = Some(Terminal::with_options(
                CrosstermBackend::new(io::stdout()),
                TerminalOptions {
                    viewport: Viewport::Fixed(self.area),
                },
            )?);
        } else {
            self.fresh_bottom()?;
        }
        while let Some(text) = self.deferred.pop_front() {
            self.deferred_bytes -= text.len();
            self.print_before(&text)?;
        }
        Ok(())
    }
    /// Release modes before foreground work or host-owned suspension.
    /// # Errors
    /// Reports cleanup failures; a failed release does not start foreground work.
    pub fn pause(mut self) -> io::Result<PausedInline> {
        #[cfg(unix)]
        let original = self.lease.as_ref().and_then(|lease| lease.original.clone());
        if self.alternate {
            self.leave_alternate()?;
        }
        self.clear_owned()?;
        self.terminal = None;
        if let Some(lease) = self.lease.take() {
            lease.close()?;
        }
        Ok(PausedInline {
            options: self.options,
            #[cfg(unix)]
            original,
        })
    }
    /// Replace temporary rows with a wrapped plain result after releasing terminal modes.
    /// An empty result explicitly clears only the owned region.
    /// # Errors
    /// Reports release and print errors together, attempting cleanup even after drawing failed.
    pub fn finish(mut self, result: &str) -> io::Result<()> {
        let mut errors = vec![];
        let was_failed = self.failed;
        self.failed = false;
        if self.alternate
            && let Err(e) = self.leave_alternate()
        {
            errors.push(e.to_string());
        }
        let cleanup = if was_failed {
            self.fresh_bottom()
        } else {
            self.clear_owned()
        };
        if let Err(e) = cleanup {
            errors.push(e.to_string());
        }
        self.terminal = None;
        if let Some(lease) = self.lease.take()
            && let Err(e) = lease.close()
        {
            errors.push(e.to_string());
        }
        if !result.is_empty()
            && let Err(e) = writeln!(io::stdout(), "{}", printable(result))
        {
            errors.push(e.to_string());
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(io::Error::other(errors.join("; ")))
        }
    }
}
impl Drop for InlineSession {
    fn drop(&mut self) {
        self.terminal = None;
        if let Some(lease) = self.lease.take() {
            let _ = lease.close();
        }
    }
}
fn printable(text: &str) -> String {
    text.chars()
        .map(|c| if c.is_control() && c != '\n' { ' ' } else { c })
        .collect()
}
