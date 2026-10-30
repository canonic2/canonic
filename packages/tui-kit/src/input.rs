//! Single-line input over caller-owned storage, such as a host's zeroizing secret buffer.
use crate::{
    event::{Event, KeyCode, KeyEventKind, KeyModifiers},
    theme::Presentation,
};
use ratatui::{
    Frame,
    layout::Rect,
    widgets::{Block, Paragraph},
};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

/// An input intent; only a deliberate unmodified Enter submits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    /// Continue editing.
    None,
    /// Submit the current buffer.
    Submit,
    /// Escape; a host may return to its previous step.
    Back,
    /// Ctrl-C or Ctrl-D cancels the flow.
    Cancel,
    /// A complete insertion was refused; the buffer is unchanged.
    Invalid,
}

/// Caret and limits only: text belongs to the host and is never retained here.
/// If the host shortens or replaces the text, the caret moves to the nearest
/// earlier grapheme boundary; construct a new field to start at the end instead.
/// An insertion beyond the buffer's capacity reallocates it; hosts holding
/// secrets reserve `limit` bytes first so no stray copy is left behind.
pub struct LineInput {
    cursor: usize,
    limit: usize,
}
impl LineInput {
    /// Start at the end of a valid, bounded single-line default.
    #[must_use]
    pub fn new(text: &str, limit: usize) -> Self {
        Self {
            cursor: text.len(),
            limit,
        }
    }
    /// Validate defaults and complete insertions without truncating them.
    #[must_use]
    pub fn valid(text: &str, limit: usize) -> bool {
        text.len() <= limit && !text.chars().any(char::is_control)
    }
    /// The caret on a grapheme boundary of `text`, which the host may have
    /// shortened or replaced since the last call.
    fn caret(&self, text: &str) -> usize {
        if self.cursor >= text.len() {
            return text.len();
        }
        text.grapheme_indices(true)
            .map(|(i, _)| i)
            .take_while(|i| *i <= self.cursor)
            .last()
            .unwrap_or(0)
    }
    fn previous(&self, text: &str) -> usize {
        text.grapheme_indices(true)
            .map(|(i, _)| i)
            .take_while(|i| *i < self.cursor)
            .last()
            .unwrap_or(0)
    }
    fn next(&self, text: &str) -> usize {
        text.grapheme_indices(true)
            .map(|(i, _)| i)
            .find(|i| *i > self.cursor)
            .unwrap_or(text.len())
    }
    fn insert(&mut self, text: &mut String, value: &str) -> Action {
        if !Self::valid(value, self.limit.saturating_sub(text.len())) {
            return Action::Invalid;
        }
        text.insert_str(self.cursor, value);
        let end = self.cursor + value.len();
        self.cursor = Self::boundary_after(text, end);
        Action::None
    }
    fn boundary_after(text: &str, position: usize) -> usize {
        text.grapheme_indices(true)
            .map(|(i, _)| i)
            .find(|i| *i >= position)
            .unwrap_or(text.len())
    }
    /// Update the supplied buffer. Paste never submits, even if it contains Enter;
    /// one trailing line break, as copied with a token, is dropped.
    pub fn update(&mut self, text: &mut String, event: &Event) -> Action {
        self.cursor = self.caret(text);
        match event {
            Event::Paste(value) => {
                let value = value
                    .strip_suffix("\r\n")
                    .or_else(|| value.strip_suffix('\n'))
                    .unwrap_or(value);
                self.insert(text, value)
            }
            Event::Key(key) if key.kind != KeyEventKind::Release => match key.code {
                KeyCode::Enter if key.kind == KeyEventKind::Press && key.modifiers.is_empty() => {
                    Action::Submit
                }
                KeyCode::Esc => Action::Back,
                KeyCode::Char('c' | 'd') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    Action::Cancel
                }
                KeyCode::Left => {
                    self.cursor = self.previous(text);
                    Action::None
                }
                KeyCode::Right => {
                    self.cursor = self.next(text);
                    Action::None
                }
                KeyCode::Home => {
                    self.cursor = 0;
                    Action::None
                }
                KeyCode::End => {
                    self.cursor = text.len();
                    Action::None
                }
                KeyCode::Backspace => {
                    let start = self.previous(text);
                    text.replace_range(start..self.cursor, "");
                    self.cursor = Self::boundary_after(text, start);
                    Action::None
                }
                KeyCode::Delete => {
                    let end = self.next(text);
                    text.replace_range(self.cursor..end, "");
                    self.cursor = Self::boundary_after(text, self.cursor);
                    Action::None
                }
                KeyCode::Char(ch) if typed(key.modifiers) => {
                    self.insert(text, ch.encode_utf8(&mut [0; 4]))
                }
                _ => Action::None,
            },
            _ => Action::None,
        }
    }
    /// Render a bounded field. Hidden text is never copied, measured or rendered.
    pub fn render(
        &self,
        frame: &mut Frame<'_>,
        area: Rect,
        text: &str,
        title: &str,
        hidden: bool,
        p: Presentation,
    ) {
        let title = crate::text::clean(title, 256, false);
        let block = Block::bordered().title(title).style(p.base());
        let inner = block.inner(area);
        frame.render_widget(block, area);
        if inner.is_empty() {
            return;
        }
        if hidden {
            frame.render_widget(Paragraph::new("[hidden]").style(p.muted()), inner);
            frame.set_cursor_position((inner.x, inner.y));
            return;
        }
        let width = usize::from(inner.width.saturating_sub(1));
        let cursor = self.caret(text);
        let mut start = cursor;
        for (index, _) in text[..cursor].grapheme_indices(true).rev() {
            if text[index..cursor].width() > width {
                break;
            }
            start = index;
        }
        let visible = crate::text::clean(&text[start..], self.limit, false);
        frame.render_widget(Paragraph::new(visible).style(p.base()), inner);
        let column = u16::try_from(text[start..cursor].width())
            .unwrap_or(u16::MAX)
            .min(inner.width - 1);
        frame.set_cursor_position((inner.x + column, inner.y));
    }
}

/// Whether a character key with these modifiers is text. Windows reports `AltGr`,
/// which types characters such as `@` and `{` on many layouts, as Ctrl+Alt.
fn typed(modifiers: KeyModifiers) -> bool {
    let chord = KeyModifiers::CONTROL | KeyModifiers::ALT;
    !modifiers.intersects(chord) || (cfg!(windows) && modifiers.contains(chord))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::KeyEvent;
    fn key(code: KeyCode) -> Event {
        Event::Key(KeyEvent::new(code, KeyModifiers::NONE))
    }
    #[test]
    fn bounded_paste_is_atomic_and_never_submits() {
        let mut text = "acme".to_owned();
        let mut field = LineInput::new(&text, 8);
        assert_eq!(
            field.update(&mut text, &Event::Paste("\nsecret".into())),
            Action::Invalid
        );
        assert_eq!(
            field.update(&mut text, &Event::Paste("12345".into())),
            Action::Invalid
        );
        assert_eq!(
            field.update(&mut text, &Event::Paste("a\n\n".into())),
            Action::Invalid
        );
        assert_eq!(text, "acme");
        assert_eq!(
            field.update(&mut text, &Event::Paste("é\r\n".into())),
            Action::None
        );
        assert_eq!(text, "acmeé");
        assert_eq!(
            field.update(&mut text, &key(KeyCode::Enter)),
            Action::Submit
        );
    }
    #[test]
    fn editing_preserves_graphemes_and_separates_back_from_cancel() {
        let mut text = "a👩‍💻e\u{301}".to_owned();
        let mut field = LineInput::new(&text, 100);
        field.update(&mut text, &key(KeyCode::Backspace));
        assert_eq!(text, "a👩‍💻");
        field.update(&mut text, &key(KeyCode::Left));
        field.update(&mut text, &key(KeyCode::Delete));
        assert_eq!(text, "a");
        assert_eq!(field.update(&mut text, &key(KeyCode::Esc)), Action::Back);
        assert_eq!(
            field.update(
                &mut text,
                &Event::Key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL))
            ),
            Action::Cancel
        );
    }
    #[test]
    fn enter_repeats_releases_and_modified_keys_never_submit() {
        use crate::event::KeyEvent;
        let mut text = String::new();
        let mut field = LineInput::new(&text, 10);
        for kind in [KeyEventKind::Repeat, KeyEventKind::Release] {
            let mut key = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
            key.kind = kind;
            assert_eq!(field.update(&mut text, &Event::Key(key)), Action::None);
        }
        assert_eq!(
            field.update(
                &mut text,
                &Event::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::ALT))
            ),
            Action::None
        );
    }
    #[test]
    fn altgr_types_text_only_where_the_platform_reports_it_as_ctrl_alt() {
        let mut text = String::new();
        let mut field = LineInput::new(&text, 10);
        let altgr = KeyEvent::new(
            KeyCode::Char('@'),
            KeyModifiers::CONTROL | KeyModifiers::ALT,
        );
        field.update(&mut text, &Event::Key(altgr));
        assert_eq!(text == "@", cfg!(windows));
        let control = KeyEvent::new(KeyCode::Char('a'), KeyModifiers::CONTROL);
        field.update(&mut text, &Event::Key(control));
        assert!(!text.contains('a'));
    }
    #[test]
    fn deletion_repairs_the_caret_when_neighboring_graphemes_join() {
        let mut text = "🇦X🇧".to_owned();
        let mut field = LineInput::new(&text, 100);
        field.update(&mut text, &key(KeyCode::Left));
        field.update(&mut text, &key(KeyCode::Backspace));
        assert_eq!(text, "🇦🇧");
        field.update(&mut text, &key(KeyCode::Backspace));
        assert_eq!(text, "");
    }
    #[test]
    fn host_replaced_text_moves_the_caret_instead_of_panicking() {
        use ratatui::{Terminal, backend::TestBackend};
        let mut text = "acme-token".to_owned();
        let mut field = LineInput::new(&text, 100);
        text.clear();
        let mut terminal = Terminal::new(TestBackend::new(20, 3)).unwrap();
        terminal
            .draw(|frame| {
                field.render(
                    frame,
                    frame.area(),
                    &text,
                    "Token",
                    false,
                    Presentation::default(),
                );
            })
            .unwrap();
        assert_eq!(
            field.update(&mut text, &Event::Paste("ok".into())),
            Action::None
        );
        assert_eq!(text, "ok");
        // A caret inside a replaced multi-byte character snaps to its start.
        let mut text = "abcd".to_owned();
        let mut field = LineInput::new(&text, 100);
        field.update(&mut text, &key(KeyCode::Left));
        text = "a界".to_owned();
        field.update(&mut text, &key(KeyCode::Backspace));
        assert_eq!(text, "界");
    }
    #[test]
    fn hidden_render_never_exposes_the_buffer_even_in_tiny_views() {
        use ratatui::{Terminal, backend::TestBackend};
        for (width, height) in [(1, 1), (12, 3), (80, 5)] {
            let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
            let text = "SECRET_TOKEN";
            let field = LineInput::new(text, 8192);
            terminal
                .draw(|frame| {
                    field.render(
                        frame,
                        frame.area(),
                        text,
                        "Token",
                        true,
                        Presentation::default(),
                    );
                })
                .unwrap();
            let rendered: String = terminal
                .backend()
                .buffer()
                .content
                .iter()
                .map(ratatui::buffer::Cell::symbol)
                .collect();
            assert!(!rendered.contains("SECRET"));
        }
    }
}
