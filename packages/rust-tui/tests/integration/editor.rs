//! Exercises Unicode editing and focus through public contracts, with real layout.

use canonic_rust_tui::{
    editor::{Editor, Outcome},
    event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers},
    focus::Focus,
    ratatui::{Terminal, backend::TestBackend},
    theme::Theme,
};
use proptest::prelude::*;
use std::time::Duration;
use unicode_segmentation::UnicodeSegmentation;

fn key(code: KeyCode, modifiers: KeyModifiers) -> Event {
    Event::Key(KeyEvent::new(code, modifiers))
}
fn press(editor: &mut Editor, code: KeyCode) {
    editor.update(&key(code, KeyModifiers::NONE));
}
fn ctrl(editor: &mut Editor, c: char) {
    editor.update(&key(KeyCode::Char(c), KeyModifiers::CONTROL));
}

#[test]
fn edits_selection_and_undo_preserve_whole_graphemes_and_cursor() {
    let mut editor = Editor::default();
    editor.set_text("a👩‍💻e\u{301}界");
    press(&mut editor, KeyCode::Home);
    press(&mut editor, KeyCode::Right);
    editor.update(&key(KeyCode::Right, KeyModifiers::SHIFT));
    assert_eq!(&editor.text()[editor.selection().unwrap()], "👩‍💻");
    editor.insert("X");
    assert_eq!(editor.text(), "aXe\u{301}界");
    ctrl(&mut editor, 'z');
    assert_eq!(editor.text(), "a👩‍💻e\u{301}界");
    assert!(editor.selection().is_some());
    ctrl(&mut editor, 'y');
    assert_eq!(editor.text(), "aXe\u{301}界");
    press(&mut editor, KeyCode::Delete);
    assert_eq!(editor.text(), "aX界");
    press(&mut editor, KeyCode::Backspace);
    assert_eq!(editor.text(), "a界");
    ctrl(&mut editor, 'z');
    assert_eq!(editor.text(), "aX界");
    editor.insert("Y");
    ctrl(&mut editor, 'y');
    assert_eq!(editor.text(), "aXY界");
}

#[test]
fn paste_is_one_transaction_and_controls_are_preserved_as_display_data() {
    let mut editor = Editor::new(128);
    editor.set_text("old");
    ctrl(&mut editor, 'g');
    let pasted = "one\r\ntwo\r界\t\x1b[31mred\x1b[0m\x1b]52;c;hidden\x07";
    assert_eq!(
        editor.update(&Event::Paste(pasted.into())),
        Outcome::Changed
    );
    assert_eq!(
        editor.text(),
        pasted.replace("\r\n", "\n").replace('\r', "\n")
    );
    ctrl(&mut editor, 'z');
    assert_eq!(editor.text(), "old");
    assert_eq!(
        editor.update(&key(KeyCode::Enter, KeyModifiers::NONE)),
        Outcome::Unhandled
    );
    ctrl(&mut editor, 'j');
    assert_eq!(editor.text(), "\n");
    let mut tiny = Editor::new(4);
    assert!(tiny.set_text("👩‍💻"));
    assert_eq!(tiny.text(), "");
    tiny.insert("ab");
    press(&mut tiny, KeyCode::Left);
    tiny.insert("\u{301}");
    assert_eq!(tiny.text(), "a\u{301}b");
    assert_eq!(tiny.cursor(), 3);
}

#[test]
fn word_navigation_and_visual_vertical_movement_have_stable_positions() {
    let mut editor = Editor::default();
    editor.set_text("hello, world");
    editor.update(&key(KeyCode::Left, KeyModifiers::CONTROL));
    assert_eq!(editor.cursor(), 7);
    editor.update(&key(KeyCode::Backspace, KeyModifiers::CONTROL));
    assert_eq!(editor.text(), "world");
    editor.set_text("12345\na\n12345");
    let mut terminal = Terminal::new(TestBackend::new(10, 8)).unwrap();
    terminal
        .draw(|f| editor.render(f, f.area(), Theme::Dark, true))
        .unwrap();
    press(&mut editor, KeyCode::Up);
    assert_eq!(editor.cursor(), 7);
    press(&mut editor, KeyCode::Up);
    assert_eq!(editor.cursor(), 5);
    press(&mut editor, KeyCode::Down);
    press(&mut editor, KeyCode::Down);
    assert_eq!(editor.cursor(), editor.text().len());
    editor.set_text("abcdEF");
    terminal = Terminal::new(TestBackend::new(4, 6)).unwrap();
    terminal
        .draw(|f| editor.render(f, f.area(), Theme::Dark, true))
        .unwrap();
    press(&mut editor, KeyCode::Up);
    assert_eq!(editor.cursor(), 2);
    press(&mut editor, KeyCode::Down);
    assert_eq!(editor.cursor(), 6);
    editor.set_text("abcd\nx");
    press(&mut editor, KeyCode::Home);
    press(&mut editor, KeyCode::Left);
    assert_eq!(editor.cursor(), 4);
    terminal
        .draw(|f| editor.render(f, f.area(), Theme::Dark, true))
        .unwrap();
    let position = terminal.backend().cursor_position();
    assert_eq!((position.x, position.y), (0, 2));
}

#[test]
fn focus_returns_to_surviving_target_and_rejects_duplicate_scope() {
    assert!(Focus::new(vec![1, 1]).is_none());
    let mut focus = Focus::new(vec![1, 2]).unwrap();
    focus.advance(false);
    assert_eq!(focus.active(), Some(&2));
    assert!(focus.push(vec![3]));
    assert!(!focus.push(vec![4, 4]));
    assert_eq!(focus.active(), Some(&3));
    focus.remove(&2);
    focus.pop();
    assert_eq!(focus.active(), Some(&1));
    focus.remove(&1);
    assert_eq!(focus.active(), None);
    focus.advance(true);
    assert!(!focus.pop());
}

#[test]
fn render_preserves_draft_at_tiny_sizes_and_only_focused_editor_sets_cursor() {
    let mut editor = Editor::default();
    editor.set_text("a界👩‍💻\nline");
    let old = editor.text().to_owned();
    let cursor = editor.cursor();
    for (w, h) in [(0, 0), (1, 1), (3, 3), (6, 5), (40, 12)] {
        let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
        terminal
            .draw(|frame| {
                editor.render(frame, frame.area(), Theme::Light, false);
            })
            .unwrap();
        assert!(!terminal.backend().cursor_visible());
        terminal
            .draw(|frame| {
                editor.render(frame, frame.area(), Theme::Dark, true);
            })
            .unwrap();
        if terminal.backend().cursor_visible() {
            let position = terminal.backend().cursor_position();
            assert!(position.x < w && position.y < h);
        }
        assert_eq!(editor.text(), old);
        assert_eq!(editor.cursor(), cursor);
    }
}

#[test]
fn selection_has_visible_style_and_reviewed_rendering() {
    let mut editor = Editor::default();
    editor.set_text("first\nsecond");
    press(&mut editor, KeyCode::Home);
    editor.update(&key(KeyCode::Right, KeyModifiers::SHIFT));
    let mut terminal = Terminal::new(TestBackend::new(24, 7)).unwrap();
    terminal
        .draw(|f| editor.render(f, f.area(), Theme::Light, true))
        .unwrap();
    assert!(
        terminal.backend().buffer()[(0, 2)]
            .modifier
            .contains(canonic_rust_tui::ratatui::style::Modifier::REVERSED)
    );
    insta::assert_snapshot!(
        "multiline_editor",
        super::components::dump(terminal.backend().buffer())
    );
}

#[test]
fn history_is_bounded_and_release_events_do_not_edit() {
    let mut editor = Editor::default();
    for _ in 0..80 {
        editor.insert("a");
    }
    for _ in 0..80 {
        ctrl(&mut editor, 'z');
    }
    assert_eq!(editor.text(), "a".repeat(16));
    let mut release = KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE);
    release.kind = KeyEventKind::Release;
    assert_eq!(editor.update(&Event::Key(release)), Outcome::Unhandled);
    for _ in 0..80 {
        ctrl(&mut editor, 'y');
    }
    assert_eq!(editor.text(), "a".repeat(80));
}

#[test]
fn paste_cuts_at_a_grapheme_and_zero_fit_keeps_selection() {
    let mut editor = Editor::new(5);
    editor.set_text("old");
    ctrl(&mut editor, 'g');
    assert_eq!(editor.insert("界界"), Outcome::Changed);
    assert_eq!(editor.text(), "界");
    assert!(editor.notice().unwrap().contains("3 bytes left out"));
    ctrl(&mut editor, 'z');
    assert_eq!(editor.text(), "old");
    assert!(editor.selection().is_some());
    let selection = editor.selection();
    assert_eq!(editor.insert("👩‍💻"), Outcome::Rejected);
    assert_eq!(editor.selection(), selection);
    assert_eq!(editor.text(), "old");
    assert!(editor.rejected());
    press(&mut editor, KeyCode::Backspace);
    assert!(!editor.rejected());
}

#[test]
fn typing_groups_break_on_pause_navigation_space_and_paste() {
    let mut editor = Editor::default();
    for c in "hello ".chars() {
        press(&mut editor, KeyCode::Char(c));
    }
    for c in "world".chars() {
        press(&mut editor, KeyCode::Char(c));
    }
    ctrl(&mut editor, 'z');
    assert_eq!(editor.text(), "hello ");
    ctrl(&mut editor, 'y');
    editor.tick(Duration::from_secs(1));
    press(&mut editor, KeyCode::Char('!'));
    ctrl(&mut editor, 'z');
    assert_eq!(editor.text(), "hello world");
    editor.insert("\nPASTE");
    ctrl(&mut editor, 'z');
    assert_eq!(editor.text(), "hello world");
    assert!(editor.notice().unwrap().contains("undid paste"));
    editor.tick(Duration::from_secs(3));
    assert_eq!(editor.notice(), None);
    press(&mut editor, KeyCode::Left);
    press(&mut editor, KeyCode::Char('X'));
    ctrl(&mut editor, 'z');
    assert_eq!(editor.text(), "hello world");
}

#[test]
fn design_keys_move_by_visible_rows_and_delete_logical_line_ranges() {
    let mut editor = Editor::default();
    editor.set_text("hello world\nnext");
    let mut terminal = Terminal::new(TestBackend::new(8, 5)).unwrap();
    terminal
        .draw(|f| editor.render(f, f.area(), Theme::Dark, true))
        .unwrap();
    ctrl(&mut editor, 'a');
    assert_eq!(editor.cursor(), 12);
    assert!(editor.selection().is_none());
    press(&mut editor, KeyCode::Up);
    ctrl(&mut editor, 'a');
    assert_eq!(editor.cursor(), 6);
    ctrl(&mut editor, 'e');
    assert_eq!(editor.cursor(), 11);
    ctrl(&mut editor, 'u');
    assert_eq!(editor.text(), "\nnext");
    ctrl(&mut editor, 'y'); // No redo without an undo.
    ctrl(&mut editor, 'g');
    assert_eq!(editor.selection(), Some(0..5));
    press(&mut editor, KeyCode::Esc);
    assert!(editor.selection().is_none());
    editor.set_text("alpha beta");
    editor.update(&key(KeyCode::Home, KeyModifiers::CONTROL));
    ctrl(&mut editor, 'e');
    assert_eq!(editor.cursor(), 6);
    terminal
        .draw(|f| editor.render(f, f.area(), Theme::Dark, true))
        .unwrap();
    let position = terminal.backend().cursor_position();
    assert_eq!((position.x, position.y), (6, 1));
    ctrl(&mut editor, 'a');
    assert_eq!(editor.cursor(), 0);
}

#[test]
fn word_layout_does_not_split_prepend_graphemes_at_whitespace() {
    let mut editor = Editor::default();
    editor.set_text("\u{11a84}\u{a0}");
    press(&mut editor, KeyCode::Left);
    press(&mut editor, KeyCode::Up);
    assert_eq!(editor.cursor(), 0);
    let mut terminal = Terminal::new(TestBackend::new(20, 5)).unwrap();
    terminal
        .draw(|frame| editor.render(frame, frame.area(), Theme::Dark, true))
        .unwrap();
    press(&mut editor, KeyCode::End);
    assert_eq!(editor.cursor(), editor.text().len());
}

proptest! {
    #[test]
    fn arbitrary_unicode_and_navigation_keep_valid_grapheme_ranges(text in ".{0,80}", actions in prop::collection::vec(0_u8..9,0..120)) {
        let mut editor = Editor::new(128); editor.set_text(&text);
        for action in actions {
            match action {
                0 => {press(&mut editor,KeyCode::Left);},
                1 => {editor.update(&key(KeyCode::Right,KeyModifiers::SHIFT));},
                2 => {press(&mut editor,KeyCode::Backspace);},
                3 => {press(&mut editor,KeyCode::Delete);},
                4 => {editor.insert("e\u{301}👩‍💻");},
                5 => {ctrl(&mut editor,'z');},
                6 => {ctrl(&mut editor,'y');},
                7 => {press(&mut editor,KeyCode::Up);},
                _ => {press(&mut editor,KeyCode::Down);},
            }
            let boundaries: Vec<_> = editor.text().grapheme_indices(true).map(|(i,_)|i).chain(std::iter::once(editor.text().len())).collect();
            prop_assert!(boundaries.contains(&editor.cursor()));
            prop_assert!(editor.text().len()<=128);
            if let Some(r) = editor.selection() {prop_assert!(boundaries.contains(&r.start) && boundaries.contains(&r.end));}
        }
    }
}
