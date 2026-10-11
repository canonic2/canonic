//! Exercises effective command maps, scoped sequences and safe palette intents.

use canonic_tui::{
    commands::{
        Command, Commands,
        bindings::{Binding, Dispatcher, Outcome, Stroke},
        remaps::{Origin, Remap},
    },
    event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers},
    picker::Intent,
};
use std::time::Duration;

fn event(c: char) -> Event {
    Event::Key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL))
}

#[test]
fn shifted_letter_and_backtab_labels_match_their_normalized_events() {
    let mut c = commands();
    c[0].bindings = vec![Binding(vec![Stroke::new(
        KeyCode::Char('a'),
        KeyModifiers::SHIFT,
    )])];
    assert_eq!(c[0].bindings[0].label(), "A");
    assert_eq!(
        Stroke::new(KeyCode::BackTab, KeyModifiers::SHIFT).label(),
        "shift+tab"
    );
    assert_eq!(
        Stroke::new(KeyCode::Char(' '), KeyModifiers::SHIFT).label(),
        "shift+space"
    );
    let mut d = Dispatcher::new(&c, Duration::from_secs(3)).unwrap();
    assert_eq!(
        d.update(&Event::Key(KeyEvent::new(
            KeyCode::Char('A'),
            KeyModifiers::SHIFT
        ))),
        Outcome::Invoke("rename".into())
    );
}

#[test]
fn continuation_hints_wait_400ms_and_expire_after_three_seconds() {
    let mut d = Dispatcher::new(&commands(), Duration::from_secs(3)).unwrap();
    d.update(&event('k'));
    assert_eq!(d.continuations(), vec![("ctrl+r".into(), "rename".into())]);
    assert!(!d.hint_visible());
    assert!(!d.tick(Duration::from_millis(399)));
    assert!(!d.hint_visible());
    assert!(!d.tick(Duration::from_millis(1)));
    assert!(d.hint_visible());
    assert!(!d.tick(Duration::from_millis(2599)));
    assert!(d.tick(Duration::from_millis(1)));
    assert!(!d.hint_visible());
    assert_eq!(d.continuations(), Vec::<(String, String)>::new());
}

#[test]
fn later_remaps_win_and_reserved_remaps_remain_visible_without_changing_keys() {
    let mut c = commands();
    let mut second = c[0].clone();
    second.id = "other".into();
    second.bindings.clear();
    c.push(second);
    let changes = [
        Remap {
            id: "other".into(),
            bindings: c[0].bindings.clone(),
            origin: Origin {
                path: "personal/keys.toml".into(),
                line: 7,
            },
        },
        Remap {
            id: "other".into(),
            bindings: vec![Binding(vec![Stroke::new(
                KeyCode::Enter,
                KeyModifiers::NONE,
            )])],
            origin: Origin {
                path: "personal/keys.toml".into(),
                line: 8,
            },
        },
    ];
    let mut view = Commands::remapped(c, &changes).unwrap();
    assert_eq!(view.descriptors()[0].bindings, Vec::<Binding>::new());
    assert_eq!(view.descriptors()[1].bindings[0].label(), "ctrl+k ctrl+r");
    assert_eq!(view.key_info("rename").unwrap().conflicts.len(), 1);
    assert_eq!(view.key_info("other").unwrap().conflicts.len(), 2);
    let mut d = Dispatcher::new(view.descriptors(), Duration::from_secs(3)).unwrap();
    d.update(&event('k'));
    assert_eq!(d.update(&event('r')), Outcome::Invoke("other".into()));
    view.set_keys(true);
    view.set_query("ctrl+k");
    assert_eq!(view.match_count(), 2);
    assert_eq!(view.selected(), Some("other"));
    assert_eq!(
        view.update(&Event::Key(KeyEvent::new(
            KeyCode::Char('o'),
            KeyModifiers::NONE
        ))),
        Intent::Inspect("other".into())
    );
    assert_eq!(
        view.update(&Event::Key(KeyEvent::new(
            KeyCode::Enter,
            KeyModifiers::NONE
        ))),
        Intent::None
    );
}

#[test]
fn keys_scope_navigation_and_filter_never_accept_newlines_or_number_selection() {
    let mut c = commands();
    let mut second = c[0].clone();
    second.id = "global".into();
    second.label = "global 2".into();
    second.bindings.clear();
    c.push(second);
    let mut view = Commands::new(c).unwrap();
    view.set_scopes(&[("rename", "workspace")]);
    view.set_keys(true);
    view.update(&Event::Key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE)));
    assert_eq!(view.selected(), Some("global"));
    view.update(&Event::Key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE)));
    assert_eq!(view.match_count(), 1);
    assert_eq!(view.selected(), Some("rename"));
    view.update(&event('j'));
    assert_eq!(view.query(), "");
    view.set_keys(false);
    view.update(&Event::Key(KeyEvent::new(
        KeyCode::Char('2'),
        KeyModifiers::NONE,
    )));
    assert_eq!(view.query(), "2");
    assert_eq!(view.selected(), Some("global"));
}
fn commands() -> Vec<Command> {
    vec![Command {
        id: "rename".into(),
        label: "rename branch".into(),
        category: "workspace".into(),
        description: "change a branch name".into(),
        aliases: vec!["/branch".into()],
        disabled: None,
        bindings: vec![Binding(vec![
            Stroke::new(KeyCode::Char('k'), KeyModifiers::CONTROL),
            Stroke::new(KeyCode::Char('r'), KeyModifiers::CONTROL),
        ])],
    }]
}

#[test]
fn sequences_consume_mismatch_paste_and_timeout_without_invocation() {
    let mut d = Dispatcher::new(&commands(), Duration::from_secs(1)).unwrap();
    assert_eq!(d.update(&event('k')), Outcome::Pending("ctrl+k".into()));
    assert_eq!(d.update(&event('r')), Outcome::Invoke("rename".into()));
    assert_eq!(d.update(&event('r')), Outcome::Unhandled);
    d.update(&event('k'));
    assert_eq!(d.update(&Event::Paste("r".into())), Outcome::Cancelled);
    d.update(&event('k'));
    assert_eq!(
        d.update(&event('x')),
        Outcome::Mismatch("ctrl+k ctrl+x isn't a command · F1 lists them".into())
    );
    d.update(&event('k'));
    assert!(!d.tick(Duration::from_millis(999)));
    assert!(d.tick(Duration::from_millis(1)));
    assert_eq!(d.update(&event('r')), Outcome::Unhandled);
    d.update(&event('k'));
    d.reset();
    assert_eq!(d.update(&event('r')), Outcome::Unhandled);
    d.update(&event('k'));
    assert_eq!(d.update(&event('c')), Outcome::Unhandled);
    assert_eq!(d.pending(), "");
    for kind in [KeyEventKind::Repeat, KeyEventKind::Release] {
        let mut k = KeyEvent::new(KeyCode::Char('k'), KeyModifiers::CONTROL);
        k.kind = kind;
        assert_eq!(d.update(&Event::Key(k)), Outcome::Unhandled);
    }
}

#[test]
fn maps_reject_conflicts_and_reserved_newlines_and_skip_disabled_commands() {
    let mut c = commands();
    let mut second = c[0].clone();
    second.id = "other".into();
    c.push(second);
    assert!(Dispatcher::new(&c, Duration::from_secs(1)).is_err());
    c[1].bindings[0].0.pop();
    assert!(Commands::new(c).is_err());
    let mut c = commands();
    c[0].bindings = vec![Binding(vec![Stroke::new(
        KeyCode::Enter,
        KeyModifiers::SHIFT,
    )])];
    assert!(Commands::new(c).is_err());
    let mut c = commands();
    c[0].disabled = Some("offline".into());
    let mut d = Dispatcher::new(&c, Duration::from_secs(1)).unwrap();
    assert_eq!(d.update(&event('k')), Outcome::Unhandled);
    let mut view = Commands::new(c).unwrap();
    assert_eq!(
        view.update(&Event::Key(KeyEvent::new(
            KeyCode::Enter,
            KeyModifiers::NONE
        ))),
        Intent::None
    );
}

#[test]
fn alias_search_and_effective_help_emit_only_unmodified_press_intents() {
    let mut view = Commands::new(commands()).unwrap();
    assert_eq!(view.descriptors()[0].bindings[0].label(), "ctrl+k ctrl+r");
    view.update(&Event::Paste("/branch".into()));
    assert_eq!(view.query(), "/branch");
    for modifiers in [
        KeyModifiers::SHIFT,
        KeyModifiers::ALT,
        KeyModifiers::CONTROL,
    ] {
        assert_eq!(
            view.update(&Event::Key(KeyEvent::new(KeyCode::Enter, modifiers))),
            Intent::None
        );
    }
    for kind in [KeyEventKind::Repeat, KeyEventKind::Release] {
        let mut k = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
        k.kind = kind;
        assert_eq!(view.update(&Event::Key(k)), Intent::None);
    }
    assert_eq!(
        view.update(&Event::Key(KeyEvent::new(
            KeyCode::Enter,
            KeyModifiers::NONE
        ))),
        Intent::Selected("rename".into())
    );
}

#[test]
fn search_classes_precede_recency_and_typing_before_draw_keeps_opening_height() {
    use canonic_tui::picker::PickerView;
    let mut entries = Vec::new();
    for (id, label, alias) in [
        ("fuzzy", "keep old logs", ""),
        ("alias", "stop session", "kill"),
        ("word", "please kill session", ""),
        ("prefix", "kill session", ""),
        ("exact", "shortcut", ""),
    ] {
        let mut c = commands().remove(0);
        c.id = id.into();
        c.label = label.into();
        c.description.clear();
        c.aliases = vec![alias.into()];
        c.bindings.clear();
        entries.push(c);
    }
    entries[4].bindings = vec![Binding(vec![Stroke::new(
        KeyCode::Char('k'),
        KeyModifiers::CONTROL,
    )])];
    let mut view = Commands::new(entries).unwrap();
    view.set_recent(&["alias", "fuzzy"]);
    let opening = view.height(
        72,
        PickerView {
            max_height: 30,
            ..PickerView::default()
        },
    );
    view.update(&Event::Paste("kill".into()));
    assert_eq!(
        view.height(
            72,
            PickerView {
                max_height: 30,
                ..PickerView::default()
            }
        ),
        opening
    );
    assert_eq!(view.selected(), Some("prefix"));
    view.update(&Event::Key(KeyEvent::new(
        KeyCode::Down,
        KeyModifiers::NONE,
    )));
    assert_eq!(view.selected(), Some("word"));
    view.update(&Event::Key(KeyEvent::new(
        KeyCode::Down,
        KeyModifiers::NONE,
    )));
    assert_eq!(view.selected(), Some("alias"));
    view.set_query("ctrl+k");
    assert_eq!(view.selected(), Some("exact"));
    view.set_query("");
    assert_eq!(view.selected(), Some("alias"));
}

#[test]
fn empty_scopes_and_free_key_evidence_are_explicit_host_contracts() {
    use canonic_tui::{
        picker::PickerView,
        ratatui::{Terminal, backend::TestBackend},
    };
    let mut view = Commands::new(commands()).unwrap();
    view.set_scope_names(&["log view"]);
    view.set_keys(true);
    let mut terminal = Terminal::new(TestBackend::new(72, 12)).unwrap();
    view.set_query("ctrl+q");
    terminal
        .draw(|f| {
            view.render(f, f.area(), PickerView::default());
        })
        .unwrap();
    let text = super::components::dump(terminal.backend().buffer());
    assert!(text.contains("no key matches"));
    assert!(!text.contains("free in every scope"));
    view.set_complete_key_map(true);
    terminal
        .draw(|f| {
            view.render(f, f.area(), PickerView::default());
        })
        .unwrap();
    assert!(
        super::components::dump(terminal.backend().buffer())
            .contains("ctrl+q is free in every scope")
    );
    view.set_query("");
    for _ in 0..2 {
        view.update(&Event::Key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE)));
    }
    assert_eq!(view.match_count(), 0);
    terminal
        .draw(|f| {
            view.render(f, f.area(), PickerView::default());
        })
        .unwrap();
    assert!(
        super::components::dump(terminal.backend().buffer()).contains("no keys in the log view")
    );
    assert_eq!(
        view.update(&Event::Key(KeyEvent::new(
            KeyCode::Enter,
            KeyModifiers::NONE
        ))),
        Intent::None
    );
}

#[test]
fn unavailable_reason_replaces_the_key_and_attribute_only_output_marks_it() {
    use canonic_tui::{
        picker::PickerView,
        ratatui::{Terminal, backend::TestBackend},
        theme::{ColorMode, Presentation},
    };
    let mut entries = commands();
    entries[0].disabled = Some("offline".into());
    let mut view = Commands::new(entries).unwrap();
    view.set_query("ctrl+k");
    assert_eq!(view.match_count(), 1);
    assert!(view.selected().is_none());
    let mut terminal = Terminal::new(TestBackend::new(72, 12)).unwrap();
    terminal
        .draw(|f| {
            view.render(
                f,
                f.area(),
                PickerView {
                    presentation: Presentation {
                        colors: ColorMode::NoColor,
                        ..Presentation::default()
                    },
                    max_height: 12,
                    ..PickerView::default()
                },
            );
        })
        .unwrap();
    let text = super::components::dump(terminal.backend().buffer());
    let row = text
        .lines()
        .find(|line| line.contains("rename branch"))
        .unwrap();
    assert!(row.contains("[-] offline"));
    assert!(!row.contains("ctrl+k"));
    assert_eq!(view.update(&key_event(KeyCode::Enter)), Intent::None);
}
fn key_event(code: KeyCode) -> Event {
    Event::Key(KeyEvent::new(code, KeyModifiers::NONE))
}

#[test]
fn boxed_palette_clears_backdrop_attributes_inside_its_panel() {
    use canonic_tui::{
        picker::PickerView,
        ratatui::{Terminal, backend::TestBackend, layout::Rect, style::Modifier, widgets::Block},
        theme::{ColorMode, Presentation},
    };
    for colors in [ColorMode::Ansi16, ColorMode::NoColor] {
        let presentation = Presentation {
            colors,
            ..Presentation::default()
        };
        let mut commands = Commands::new(commands()).unwrap();
        let mut terminal = Terminal::new(TestBackend::new(80, 20)).unwrap();
        terminal
            .draw(|frame| {
                frame.render_widget(
                    Block::default().style(presentation.base().add_modifier(Modifier::DIM)),
                    frame.area(),
                );
                commands.render(
                    frame,
                    Rect::new(4, 2, 72, 12),
                    PickerView {
                        presentation,
                        max_height: 12,
                        ..PickerView::default()
                    },
                );
            })
            .unwrap();
        let buffer = terminal.backend().buffer();
        assert!(buffer[(0, 0)].modifier.contains(Modifier::DIM));
        assert!(
            !buffer[(4, 2)]
                .modifier
                .intersects(Modifier::DIM | Modifier::REVERSED)
        );
        assert!(
            !buffer[(75, 3)]
                .modifier
                .intersects(Modifier::DIM | Modifier::REVERSED)
        );
    }
}
