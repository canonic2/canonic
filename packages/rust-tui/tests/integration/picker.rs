//! Exercises rich picker identities, filtering, selection semantics and compact geometry.

use canonic_rust_tui::{
    event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers},
    picker::{Choice, ChoicePicker, Filter, Intent, PickerView, SelectionMode, Status},
    ratatui::{
        Terminal,
        backend::TestBackend,
        layout::Rect,
        style::{Color, Modifier},
    },
    theme::{ColorMode, Presentation, Symbols},
};
use proptest::prelude::*;

fn key(code: KeyCode) -> Event {
    Event::Key(KeyEvent::new(code, KeyModifiers::NONE))
}
fn choices() -> Vec<Choice> {
    ["Alpha", "Beta", "Gamma"]
        .into_iter()
        .enumerate()
        .map(|(i, label)| Choice::new(format!("id-{i}"), label))
        .collect()
}
fn picker(filter: Filter, mode: SelectionMode) -> ChoicePicker {
    ChoicePicker::new(choices(), Some("id-2"), mode, filter).expect("fixture identities are unique")
}

#[test]
fn disabled_danger_and_duplicate_rows_have_safe_identity_contracts() {
    let mut rows = choices();
    rows[0].danger = true;
    rows[1].disabled = Some("unavailable".into());
    let mut p = ChoicePicker::new(
        rows.clone(),
        Some("id-0"),
        SelectionMode::Single,
        Filter::Off,
    )
    .unwrap();
    assert_eq!(p.selected(), Some("id-2"));
    assert_eq!(p.update(&key(KeyCode::Char('2'))), Intent::None);
    assert_eq!(p.selected(), Some("id-2"));
    p.update(&key(KeyCode::Home));
    assert_eq!(p.selected(), Some("id-0"));
    p.update(&key(KeyCode::Down));
    assert_eq!(p.selected(), Some("id-2"));
    assert_eq!(
        p.update(&key(KeyCode::Char('1'))),
        Intent::Selected("id-0".into())
    );
    rows.push(rows[0].clone());
    assert!(p.replace_choices(rows).is_err());
    assert_eq!(p.choices().len(), 3);
    assert_eq!(p.selected(), Some("id-0"));
    let mut reordered = choices();
    reordered.swap(0, 2);
    p.replace_choices(reordered).unwrap();
    assert_eq!(
        p.update(&key(KeyCode::Enter)),
        Intent::Selected("id-0".into())
    );
    p.replace_choices(vec![Choice::new("other", "Another")])
        .unwrap();
    assert_eq!(p.selected(), Some("other"));
}

#[test]
fn fuzzy_query_edits_graphemes_paste_is_atomic_and_esc_clears_before_cancel() {
    let mut p = picker(Filter::On, SelectionMode::Single);
    assert_eq!(p.update(&Event::Paste("Gma\n".into())), Intent::None);
    assert_eq!(p.query(), "Gma");
    assert_eq!(p.selected(), Some("id-2"));
    assert_eq!(
        p.update(&key(KeyCode::Enter)),
        Intent::Selected("id-2".into())
    );
    p.set_query("Cafe\u{301}👩‍💻");
    p.update(&key(KeyCode::Backspace));
    assert_eq!(p.query(), "Cafe\u{301}");
    p.update(&key(KeyCode::Backspace));
    assert_eq!(p.query(), "Caf");
    p.update(&Event::Paste("123".into()));
    p.update(&Event::Key(KeyEvent::new(
        KeyCode::Char('z'),
        KeyModifiers::CONTROL,
    )));
    assert_eq!(p.query(), "Caf");
    assert_eq!(p.update(&key(KeyCode::Esc)), Intent::None);
    assert_eq!(p.query(), "");
    assert_eq!(p.update(&key(KeyCode::Esc)), Intent::Cancelled);
    p.set_query("not found");
    assert_eq!(p.selected(), None);
    assert_eq!(p.update(&key(KeyCode::Enter)), Intent::None);
    p.update(&key(KeyCode::Char('1')));
    assert!(p.query().ends_with('1'));

    // Label matches outrank detail matches even at the end of long labels.
    let mut detail = Choice::new("detail", "Other");
    detail.detail = "zzz".into();
    let label = Choice::new("label", format!("{}zzz", "x".repeat(3000)));
    p.replace_choices(vec![detail, label]).unwrap();
    p.set_query("zzz");
    assert_eq!(p.selected(), Some("label"));
}

#[test]
fn multi_selection_keeps_hidden_marks_and_confirms_in_source_order() {
    let mut p = picker(Filter::On, SelectionMode::Multiple);
    p.set_marked(&["id-0"]);
    p.set_query("Gma");
    p.update(&key(KeyCode::Char(' ')));
    assert_eq!(p.marked(), vec!["id-0", "id-2"]);
    assert_eq!(
        p.update(&key(KeyCode::Enter)),
        Intent::Confirmed(vec!["id-0".into(), "id-2".into()])
    );
    p.update(&Event::Key(KeyEvent::new(
        KeyCode::Char('a'),
        KeyModifiers::ALT,
    )));
    assert_eq!(p.marked(), vec!["id-0"]);
    let repeat = Event::Key(KeyEvent::new_with_kind(
        KeyCode::Char(' '),
        KeyModifiers::NONE,
        KeyEventKind::Repeat,
    ));
    p.update(&repeat);
    assert_eq!(p.query(), "Gma");
    assert_eq!(p.marked(), vec!["id-0"]);
    let mut rows = choices();
    rows[0].disabled = Some("changed availability".into());
    p.replace_choices(rows).unwrap();
    assert_eq!(p.marked(), [] as [&str; 0]);
}

#[test]
fn non_ready_empty_and_repeat_events_never_confirm_or_leak_into_query() {
    let mut p = picker(Filter::On, SelectionMode::Single);
    for status in [
        Status::Loading("pending".into()),
        Status::Error("failed".into()),
    ] {
        p.set_status(status);
        assert_eq!(p.update(&key(KeyCode::Enter)), Intent::None);
        assert_eq!(p.update(&Event::Paste("Gamma".into())), Intent::None);
        assert_eq!(p.query(), "");
        assert_eq!(p.update(&key(KeyCode::Esc)), Intent::Cancelled);
    }
    p.set_status(Status::Ready);
    for kind in [KeyEventKind::Repeat, KeyEventKind::Release] {
        assert_eq!(
            p.update(&Event::Key(KeyEvent::new_with_kind(
                KeyCode::Enter,
                KeyModifiers::NONE,
                kind
            ))),
            Intent::None
        );
    }
    assert_eq!(
        p.update(&Event::Key(KeyEvent::new(
            KeyCode::Enter,
            KeyModifiers::ALT
        ))),
        Intent::None
    );
    assert_eq!(
        p.update(&key(KeyCode::F(3))),
        Intent::Inspect("id-2".into())
    );
    p.replace_choices(vec![]).unwrap();
    assert_eq!(p.update(&key(KeyCode::Enter)), Intent::None);
    assert_eq!(p.update(&key(KeyCode::Tab)), Intent::None);
}

#[test]
fn compact_rendering_keeps_height_and_label_columns_without_side_borders() {
    let mut p = picker(Filter::Off, SelectionMode::Single);
    let presentation = Presentation {
        colors: ColorMode::TrueColor,
        ..Presentation::default()
    };
    let view = PickerView {
        title: "github server",
        presentation,
        ..PickerView::default()
    };
    let mut terminal = Terminal::new(TestBackend::new(64, 10)).unwrap();
    let mut used = Rect::default();
    terminal
        .draw(|f| used = p.render(f, f.area(), view, true))
        .unwrap();
    assert_eq!(used.height, 5);
    let buffer = terminal.backend().buffer();
    assert_eq!(buffer[(4, 1)].symbol(), "A");
    assert_eq!(buffer[(4, 3)].symbol(), "G");
    assert_eq!(Some(buffer[(0, 3)].bg), presentation.tokens().surface);
    assert!(buffer[(1, 3)].modifier.contains(Modifier::REVERSED));
    assert!(buffer[(4, 3)].modifier.contains(Modifier::BOLD));
    assert!(!terminal.backend().cursor_visible());
    insta::assert_snapshot!("option_picker", super::components::dump(buffer));
    let mut filtered = picker(Filter::On, SelectionMode::Single);
    terminal
        .draw(|f| used = filtered.render(f, f.area(), view, true))
        .unwrap();
    let before = used.height;
    filtered.set_query("nothing");
    terminal
        .draw(|f| used = filtered.render(f, f.area(), view, true))
        .unwrap();
    assert_eq!(used.height, before);
    assert!(terminal.backend().cursor_visible());
    assert!(super::components::dump(terminal.backend().buffer()).contains("No choices match"));
}

#[test]
fn grouped_paging_tracks_body_rows_and_resize_keeps_the_highlight_visible() {
    let rows = (0..30)
        .map(|i| {
            let mut c = Choice::new(i.to_string(), format!("Choice {i}"));
            c.group = format!("group {}", i / 3);
            c
        })
        .collect();
    let mut p = ChoicePicker::new(rows, Some("0"), SelectionMode::Single, Filter::On).unwrap();
    let mut terminal = Terminal::new(TestBackend::new(40, 12)).unwrap();
    terminal
        .draw(|f| {
            p.render(f, f.area(), PickerView::default(), true);
        })
        .unwrap();
    p.update(&key(KeyCode::PageDown));
    // Six visible body rows include group headings: row 1 -> row 7 is choice 5.
    assert_eq!(p.selected(), Some("5"));
    p.update(&key(KeyCode::End));
    for (width, height) in [(1, 1), (18, 8), (64, 20)] {
        terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal
            .draw(|f| {
                p.render(f, f.area(), PickerView::default(), true);
            })
            .unwrap();
        assert_eq!(p.selected(), Some("29"));
        if width > 1 {
            assert!(super::components::dump(terminal.backend().buffer()).contains("Choice 29"));
        }
    }
    p.update(&key(KeyCode::Down));
    assert_eq!(p.selected(), Some("29"));
    p.update(&key(KeyCode::Home));
    p.update(&key(KeyCode::Up));
    assert_eq!(p.selected(), Some("0"));
}

#[test]
fn narrow_ascii_no_color_profiles_drop_details_and_keep_matched_unicode_safe() {
    let mut c = Choice::new("one", "Cafe\u{301} 界 👩‍💻 very long label");
    c.detail = "detail metadata".into();
    c.current = true;
    let mut p = ChoicePicker::new(vec![c], None, SelectionMode::Single, Filter::On).unwrap();
    p.set_query("Cafe\u{301}");
    let mut terminal = Terminal::new(TestBackend::new(16, 8)).unwrap();
    let view = PickerView {
        presentation: Presentation {
            colors: ColorMode::NoColor,
            symbols: Symbols::Ascii,
            ..Presentation::default()
        },
        ..PickerView::default()
    };
    terminal
        .draw(|f| {
            p.render(f, f.area(), view, true);
        })
        .unwrap();
    let text = super::components::dump(terminal.backend().buffer());
    assert!(!text.contains("detail metadata"));
    assert!(text.contains("Cafe\u{301}"));
    assert!(text.contains('.'));
    assert!(
        terminal.backend().buffer()[(2, 2)]
            .modifier
            .contains(Modifier::UNDERLINED)
    );
    for cell in &terminal.backend().buffer().content {
        assert_eq!((cell.fg, cell.bg), (Color::Reset, Color::Reset));
    }
    for (width, height) in [(0, 0), (1, 1), (2, 2)] {
        terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal
            .draw(|f| {
                p.render(f, f.area(), view, true);
            })
            .unwrap();
    }
}

proptest! {
    #[test]
    fn unicode_filter_and_navigation_only_return_supplied_enabled_identities(query in "[a-zé界👩‍💻]{0,30}", moves in prop::collection::vec(any::<bool>(), 0..50)) {
        let mut p = picker(Filter::On, SelectionMode::Single); p.set_query(&query);
        for down in moves { p.update(&key(if down { KeyCode::Down } else { KeyCode::Up })); }
        if let Intent::Selected(id) = p.update(&key(KeyCode::Enter)) { prop_assert!(p.choices().iter().any(|c| c.id == id && c.disabled.is_none())); }
        prop_assert!(p.query().len() <= 4096);
    }
}
