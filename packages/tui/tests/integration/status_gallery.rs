//! Gallery placement and caller restoration through public event/render APIs.
use canonic_tui::{
    event::{Event, KeyCode, KeyEvent, KeyModifiers},
    gallery::{Component, Gallery},
    ratatui::{Terminal, backend::TestBackend},
};
fn key(code: KeyCode, modifiers: KeyModifiers) -> Event {
    Event::Key(KeyEvent::new(code, modifiers))
}
fn screen(g: &mut Gallery, width: u16, height: u16) -> String {
    let mut t =
        Terminal::new(TestBackend::new(width, height)).expect("valid status fixture or operation");
    t.draw(|f| g.render(f))
        .expect("valid status fixture or operation");
    super::components::dump(t.backend().buffer())
}
#[test]
fn status_gallery_states_profiles_and_responsive_return_are_live() {
    let mut g = Gallery::new();
    for (name, expected) in [
        ("main", "Changed files"),
        ("unknown", "not reported"),
        ("loading", "waiting for host"),
        ("empty", "no entries"),
        ("error", "host unavailable"),
        ("notices", "Local connection"),
        ("pending", "waiting for host"),
        ("action-failed", "Host could not"),
        ("set-aside", "set aside"),
        ("resolved", "Context nearing"),
        ("expired", "Local connection"),
        ("disabled", "Host policy"),
    ] {
        g.open_scenario(Component::StatusPanelsAndNotices, name)
            .expect("valid status fixture or operation");
        let text = screen(&mut g, 130, 40);
        assert!(text.contains(expected), "{name}: {text}");
        assert!(!text.contains("design pending"));
    }
    for name in [
        "medium",
        "narrow",
        "tiny",
        "ascii",
        "ansi16",
        "no-color",
        "light",
        "reduced-motion",
        "review",
        "narrow-40",
        "no-color-ascii",
    ] {
        g.open_scenario(Component::StatusPanelsAndNotices, name)
            .expect("valid status fixture or operation");
        assert!(!screen(&mut g, 64, 26).contains('\x1b'));
    }
    g.open_scenario(Component::StatusPanelsAndNotices, "main")
        .expect("valid status fixture or operation");
    for width in [130, 64, 40] {
        screen(&mut g, width, 40);
        g.update(&key(KeyCode::Char('x'), KeyModifiers::CONTROL));
        g.update(&key(KeyCode::Char('i'), KeyModifiers::NONE));
        let text = screen(&mut g, width, 40);
        assert!(text.contains("Context"));
        assert!(text.contains("and bump the app version"));
        g.update(&key(KeyCode::Esc, KeyModifiers::NONE));
    }
    g.update(&key(KeyCode::Char('!'), KeyModifiers::NONE));
    assert!(screen(&mut g, 64, 40).contains("and bump the app versio!n"));
}
#[test]
fn status_gallery_snapshots_cover_wide_narrow_notices_and_failure() {
    for (name, width) in [
        ("main", 130),
        ("narrow", 40),
        ("notices", 80),
        ("action-failed", 80),
        ("ascii", 130),
        ("review", 130),
    ] {
        let mut g = Gallery::new();
        g.open_scenario(Component::StatusPanelsAndNotices, name)
            .expect("valid status fixture or operation");
        insta::assert_snapshot!(format!("status_{name}"), screen(&mut g, width, 40));
    }
}

#[test]
fn pane_leader_expires_without_switching_focus_or_editing_the_saved_draft() {
    let mut g = Gallery::new();
    g.open_scenario(Component::StatusPanelsAndNotices, "medium")
        .expect("live fixture");
    g.update(&key(KeyCode::Char('x'), KeyModifiers::CONTROL));
    for _ in 0..20 {
        g.tick();
    }
    g.update(&key(KeyCode::Char('n'), KeyModifiers::NONE));
    let text = screen(&mut g, 64, 40);
    assert!(text.contains("Status"));
    assert!(!text.contains("Notices"));
    assert!(text.contains("and bump the app version"));
}
