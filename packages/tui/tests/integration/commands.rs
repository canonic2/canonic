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
