//! Reused component rendering and retained draft across borrowed views.
use canonic_tui::{
    event::{Event, KeyCode, KeyEvent, KeyModifiers},
    gallery::{Component, Gallery},
    ratatui::{Terminal, backend::TestBackend},
};
fn key(code: KeyCode, modifiers: KeyModifiers) -> Event {
    Event::Key(KeyEvent::new(code, modifiers))
}
fn screen(g: &mut Gallery, width: u16, height: u16) -> String {
    let mut t = Terminal::new(TestBackend::new(width, height)).unwrap();
    t.draw(|f| g.render(f)).unwrap();
    super::components::dump(t.backend().buffer())
}
#[test]
fn inline_states_render_and_borrowed_views_preserve_the_caret() {
    let mut g = Gallery::new();
    for (name, expected) in [
        ("main", "Deploy"),
        ("streaming", "writing"),
        ("out-of-order", "earlier card"),
        ("follow-up", "disconnected"),
        ("pending", "sending Allow once"),
        ("refused", "host plan changed"),
        ("withdrawn", "withdrawn by host"),
        ("plain", "no default"),
    ] {
        g.open_scenario(Component::InlineComposition, name).unwrap();
        let text = screen(&mut g, 100, 40);
        assert!(text.contains(expected), "{name}: {text}");
    }
    g.open_scenario(Component::InlineComposition, "streaming")
        .unwrap();
    screen(&mut g, 100, 40);
    for letter in ['o', 'f', 'i'] {
        g.update(&key(KeyCode::Char('x'), KeyModifiers::CONTROL));
        g.update(&key(KeyCode::Char(letter), KeyModifiers::NONE));
        screen(&mut g, 100, 40);
        g.update(&key(KeyCode::Esc, KeyModifiers::NONE));
        assert!(screen(&mut g, 100, 40).contains("and bump the app version"));
    }
    g.update(&key(KeyCode::Char('!'), KeyModifiers::NONE));
    assert!(screen(&mut g, 100, 40).contains("and bump the app versio!n"));
    g.open_scenario(Component::InlineComposition, "no-alternate")
        .unwrap();
    screen(&mut g, 100, 40);
    g.update(&key(KeyCode::Char('f'), KeyModifiers::CONTROL));
    g.update(&key(KeyCode::Char('!'), KeyModifiers::NONE));
    let text = screen(&mut g, 100, 40);
    assert!(text.contains("needs alternate screen"));
    assert!(text.contains("and bump the app versio!n"));
}
#[test]
fn inline_snapshots_cover_printed_live_dock_and_failures() {
    for (name, width, height) in [
        ("main", 80, 24),
        ("streaming", 100, 32),
        ("out-of-order", 80, 24),
        ("follow-up", 80, 24),
        ("pending", 80, 24),
        ("refused", 40, 24),
        ("tiny", 18, 6),
    ] {
        let mut g = Gallery::new();
        g.open_scenario(Component::InlineComposition, name).unwrap();
        insta::assert_snapshot!(format!("inline_{name}"), screen(&mut g, width, height));
    }
}
