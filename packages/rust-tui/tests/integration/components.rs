//! Exercises public component behavior independently of a terminal or product.

use canonic_rust_tui::{
    chat::{self, Chat},
    event::{Event, KeyCode, KeyEvent, KeyModifiers},
    picker::{self, Picker},
    ratatui::{Terminal, backend::TestBackend, layout::Rect},
    theme::Theme,
};
use proptest::prelude::*;

fn key(code: KeyCode) -> Event {
    Event::Key(KeyEvent::new(code, KeyModifiers::NONE))
}

#[test]
fn picker_confirms_only_an_explicit_valid_selection_and_cancel_does_not_select() {
    let mut picker = Picker::new(
        "Approval",
        vec!["Allow once".into(), "Cancel".into()],
        Some(1),
    );
    assert_eq!(
        picker.update(&key(KeyCode::Enter)),
        picker::Action::Selected(1)
    );
    assert_eq!(picker.update(&key(KeyCode::Esc)), picker::Action::Cancelled);
    let mut invalid = Picker::new("Approval", vec!["Allow once".into()], Some(9));
    assert_eq!(invalid.update(&key(KeyCode::Enter)), picker::Action::None);
    let mut empty = Picker::new("Empty", Vec::new(), None);
    empty.update(&key(KeyCode::Down));
    assert_eq!(empty.update(&key(KeyCode::Enter)), picker::Action::None);
}

#[test]
fn search_retains_original_choice_identity_and_paste_never_confirms() {
    let mut picker = Picker::new(
        "Choose",
        vec!["Alpha".into(), "Beta".into(), "Gamma".into()],
        Some(0),
    );
    assert_eq!(
        picker.update(&Event::Paste("Gamma\n".into())),
        picker::Action::None
    );
    assert_eq!(
        picker.update(&key(KeyCode::Enter)),
        picker::Action::Selected(2)
    );
    picker.update(&Event::Paste("no match".into()));
    assert_eq!(picker.selected(), None);
    assert_eq!(picker.update(&key(KeyCode::Enter)), picker::Action::None);
}

#[test]
fn chat_paste_multiline_graphemes_and_cancel_are_product_independent() {
    let mut chat = Chat::new();
    assert_eq!(
        chat.update(&Event::Paste("Hello\n👩‍💻".into())),
        chat::Action::None
    );
    chat.update(&key(KeyCode::Backspace));
    assert_eq!(chat.input(), "Hello\n");
    assert_eq!(
        chat.update(&key(KeyCode::Enter)),
        chat::Action::Submit("Hello\n".into())
    );
    assert_eq!(chat.input(), "");
    chat.set_input("kept draft");
    assert_eq!(
        chat.update(&Event::Key(KeyEvent::new(
            KeyCode::Char('x'),
            KeyModifiers::CONTROL
        ))),
        chat::Action::Cancel
    );
    assert_eq!(chat.input(), "kept draft");
}

#[test]
fn history_and_input_are_bounded_and_control_sequences_are_not_rendered() {
    let mut chat = Chat::new();
    for _ in 0..120 {
        chat.push("Acme", "A message");
    }
    assert_eq!(chat.message_count(), 100);
    chat.set_input(&"界".repeat(3000));
    assert!(chat.input().len() <= 4096);
    chat.set_input("hello\x1b[31m");
    assert!(chat.input().contains('\x1b'));
    let mut terminal = Terminal::new(TestBackend::new(40, 12)).unwrap();
    terminal
        .draw(|frame| chat.render(frame, frame.area(), Theme::Dark))
        .unwrap();
    let rendered = dump(terminal.backend().buffer());
    assert!(rendered.contains("hello␛[31m"));
    assert!(!rendered.contains('\x1b'));
}

#[test]
fn components_render_at_tiny_sizes_and_chat_follows_wrapped_unicode_output() {
    let picker = Picker::new("Picker", Vec::new(), None);
    let mut chat = Chat::new();
    chat.push("Acme", &"界🙂 long line ".repeat(40));
    chat.push("Acme", "LAST MESSAGE");
    for (width, height) in [(0, 0), (1, 1), (12, 8), (40, 20)] {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal
            .draw(|frame| picker.render(frame, frame.area(), Theme::Dark))
            .unwrap();
        terminal
            .draw(|frame| chat.render(frame, frame.area(), Theme::Light))
            .unwrap();
        if width == 40 {
            assert!(dump(terminal.backend().buffer()).contains("LAST MESSAGE"));
        }
    }
}

pub(super) fn dump(buffer: &canonic_rust_tui::ratatui::buffer::Buffer) -> String {
    let mut output = String::new();
    for y in 0..buffer.area.height {
        let mut line = String::new();
        for x in 0..buffer.area.width {
            line.push_str(buffer[(x, y)].symbol());
        }
        output.push_str(line.trim_end());
        output.push('\n');
    }
    output
}

proptest! {
    #[test]
    fn arbitrary_navigation_keeps_choice_indices_valid(moves in prop::collection::vec(any::<bool>(), 0..200), size in 0_usize..20) {
        let mut picker = Picker::new("Choices", (0..size).map(|n| n.to_string()).collect(), None);
        for down in moves { picker.update(&key(if down { KeyCode::Down } else { KeyCode::Up })); }
        prop_assert!(picker.selected().is_none_or(|index| index < size));
    }
}

#[test]
fn selected_choice_uses_the_theme_focus_style() {
    let picker = Picker::new("Choose", vec!["Safe default".into()], Some(0));
    let mut terminal = Terminal::new(TestBackend::new(40, 8)).unwrap();
    terminal
        .draw(|frame| picker.render(frame, Rect::new(0, 0, 40, 8), Theme::Light))
        .unwrap();
    let buffer = terminal.backend().buffer();
    assert!(dump(buffer).contains("> Safe default"));
    assert_eq!(buffer[(1, 2)].fg, Theme::Light.focus().fg.unwrap());
}
