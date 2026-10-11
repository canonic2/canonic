//! Verifies design profiles, host props, copy-safe geometry and message timing.

use canonic_tui::{
    editor::{Editor, EditorView},
    event::{Event, KeyCode, KeyEvent, KeyModifiers},
    ratatui::{
        Terminal,
        backend::TestBackend,
        style::{Color, Modifier},
    },
    theme::{ColorMode, Presentation, Symbols, Theme},
};
use std::time::Duration;

#[test]
fn profiles_preserve_text_and_use_tint_or_reverse_without_side_borders() {
    for mode in [
        ColorMode::Ansi16,
        ColorMode::Ansi256,
        ColorMode::TrueColor,
        ColorMode::NoColor,
    ] {
        for theme in [Theme::Dark, Theme::Light] {
            let p = Presentation {
                colors: mode,
                theme,
                symbols: Symbols::Unicode,
            };
            let mut editor = Editor::default();
            editor.set_text("first\nsecond");
            editor.update(&Event::Key(KeyEvent::new(
                KeyCode::Home,
                KeyModifiers::NONE,
            )));
            editor.update(&Event::Key(KeyEvent::new(
                KeyCode::Right,
                KeyModifiers::SHIFT,
            )));
            let mut terminal = Terminal::new(TestBackend::new(40, 8)).unwrap();
            terminal
                .draw(|frame| {
                    editor.render_with(
                        frame,
                        frame.area(),
                        EditorView {
                            title: "notes",
                            presentation: p,
                            ..EditorView::default()
                        },
                        true,
                    );
                })
                .unwrap();
            let buffer = terminal.backend().buffer();
            assert_eq!(buffer[(0, 0)].fg, p.tokens().accent.unwrap_or(Color::Reset));
            assert!(buffer[(0, 0)].modifier.contains(Modifier::BOLD));
            assert_eq!(
                buffer[(10, 0)].fg,
                p.tokens().accent.unwrap_or(Color::Reset)
            );
            assert_eq!(buffer[(0, 1)].symbol(), "f");
            assert_eq!(buffer[(0, 2)].symbol(), "s");
            let selected = &buffer[(0, 2)];
            if matches!(mode, ColorMode::Ansi16 | ColorMode::NoColor) {
                assert!(selected.modifier.contains(Modifier::REVERSED));
            } else {
                assert_eq!(Some(selected.bg), p.tokens().selection);
            }
            if mode == ColorMode::NoColor {
                for cell in &buffer.content {
                    assert_eq!((cell.fg, cell.bg), (Color::Reset, Color::Reset));
                }
            }
            assert_eq!(editor.text(), "first\nsecond");
            terminal
                .draw(|frame| {
                    editor.render_with(
                        frame,
                        frame.area(),
                        EditorView {
                            title: "notes",
                            presentation: p,
                            ..EditorView::default()
                        },
                        false,
                    );
                })
                .unwrap();
            assert!(!terminal.backend().cursor_visible());
            assert_eq!(
                terminal.backend().buffer()[(0, 0)].fg,
                p.tokens().dim.unwrap_or(Color::Reset)
            );
            let text = super::components::dump(terminal.backend().buffer());
            assert!(!text.contains("ln 2:2"));
        }
    }
}

#[test]
fn placeholder_footer_and_transient_messages_are_host_separated() {
    let mut editor = Editor::default();
    let mut terminal = Terminal::new(TestBackend::new(64, 8)).unwrap();
    let view = EditorView {
        title: "release notes",
        placeholder: "Write notes",
        footer: "enter sends",
        ..EditorView::default()
    };
    terminal
        .draw(|frame| editor.render_with(frame, frame.area(), view, true))
        .unwrap();
    let text = super::components::dump(terminal.backend().buffer());
    assert!(text.contains("Write notes") && text.contains("enter sends"));
    assert_eq!(editor.text(), "");
    editor.insert("one\ntwo");
    terminal
        .draw(|frame| editor.render_with(frame, frame.area(), view, true))
        .unwrap();
    let text = super::components::dump(terminal.backend().buffer());
    assert!(
        text.contains("pasted 2 lines")
            && !text.contains("Write notes")
            && !text.contains("enter sends")
    );
    editor.tick(Duration::from_secs(3));
    terminal
        .draw(|frame| editor.render_with(frame, frame.area(), view, true))
        .unwrap();
    assert!(super::components::dump(terminal.backend().buffer()).contains("enter sends"));
    assert!(
        !terminal.backend().buffer()[(0, 1)]
            .modifier
            .contains(Modifier::REVERSED)
    );
}

#[test]
fn prose_wraps_by_word_and_ascii_control_display_never_emits_controls() {
    let mut editor = Editor::default();
    editor.set_text("alpha beta");
    let mut terminal = Terminal::new(TestBackend::new(8, 6)).unwrap();
    terminal
        .draw(|frame| editor.render(frame, frame.area(), Theme::Dark, true))
        .unwrap();
    assert_eq!(terminal.backend().buffer()[(0, 2)].symbol(), "b");
    editor.set_text("abc\x1b[31m\tend");
    terminal = Terminal::new(TestBackend::new(40, 8)).unwrap();
    let view = EditorView {
        presentation: Presentation {
            colors: ColorMode::NoColor,
            symbols: Symbols::Ascii,
            ..Presentation::default()
        },
        ..EditorView::default()
    };
    terminal
        .draw(|frame| editor.render_with(frame, frame.area(), view, true))
        .unwrap();
    let text = super::components::dump(terminal.backend().buffer());
    assert!(text.contains("abc^[[31m"));
    assert!(text.is_ascii());
    assert!(!text.contains('\x1b'));
    assert!(editor.text().contains('\x1b'));
}
