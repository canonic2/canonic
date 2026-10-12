//! Live session states, deterministic host results and retained navigation.

use canonic_tui::{
    event::{Event, KeyCode, KeyEvent, KeyModifiers},
    gallery::{Component, Gallery},
    ratatui::{Terminal, backend::TestBackend},
};
fn key(code: KeyCode) -> Event {
    Event::Key(KeyEvent::new(code, KeyModifiers::NONE))
}
fn screen(g: &mut Gallery, w: u16, h: u16) -> String {
    let mut t = Terminal::new(TestBackend::new(w, h)).unwrap();
    t.draw(|f| g.render(f)).unwrap();
    super::components::dump(t.backend().buffer())
}
#[test]
fn named_session_states_are_real_interactions_and_host_acknowledgements() {
    let mut g = Gallery::new();
    for (name, expected) in [
        ("main", "Search reindex"),
        ("current", "back to it"),
        ("loading", "loading sessions"),
        ("empty", "No conversations"),
        ("load-failed", "index locked"),
        ("search", "search ret"),
        ("search-all", "searching all"),
        ("all-results", "archived"),
        ("no-matches", "No loaded session"),
        ("refreshed", "Release notes"),
        ("removed", "removed elsewhere"),
        ("long-list", "more"),
        ("actions", "actions"),
        ("unavailable", "only one project"),
        ("rename", "save"),
        ("rename-pending", "renaming"),
        ("rename-refused", "session is still running"),
        ("forked", "forked"),
        ("move", "move to"),
        ("pinned", "unpin"),
        ("action-failed", "still running"),
        ("delete-confirm", "Keep it"),
        ("delete-armed", "enter again"),
        ("deleting", "deleting"),
        ("deleted", "deleted webhook"),
        ("delete-failed", "still running"),
        ("last-deleted", "No conversations"),
        ("narrow", "acme-api"),
        ("narrow-search", "search ret"),
        ("narrow-delete", "Keep it"),
        ("medium", "Search reindex"),
        ("ascii", "*"),
        ("ansi16", "needs you"),
        ("no-color", "Search reindex"),
        ("light", "Search reindex"),
        ("no-color-pending", "deleting"),
        ("before-chat", "and then open the PR"),
        ("returning", "and then open the PR"),
        ("switched", "Stuck retries conversation"),
        ("switch-back", "and then open the PR"),
        ("open-failed", "still running"),
        ("paging", "loading sessions"),
        ("page-failed", "explicit retry"),
        ("page-retry", "loading sessions"),
        ("search-failed", "search failed"),
        ("search-retry", "searching all"),
        ("stale-search", "stale result rejected: true"),
        ("foreign-search", "foreign result rejected: true"),
        ("stale-action", "stale result rejected: true"),
        ("foreign-action", "foreign result rejected: true"),
        ("duplicate-result", "stale result rejected: true"),
        ("material-changed", "updated by host"),
        ("action-removed", "removed elsewhere"),
        ("concurrent", "pinning"),
        ("pending-return", "and then open the PR"),
        ("delayed", "forking"),
        ("no-ack", "forking"),
        ("long-title", "long session"),
        ("long-preview", "preview line"),
        ("long-delete", "Delete this conversation"),
        ("long-destinations", "Acme project"),
        ("rename-limit", "over the limit title"),
        ("paste-isolated", "save"),
        ("reduced-motion", "forking"),
        ("short", "sessions"),
        ("streaming", "sessions"),
        ("rename-replaced", "rename cancelled"),
        ("delete-expired", "Keep it"),
        ("delete-disarmed", "Keep it"),
        ("palette", "Sessions"),
        ("queued-drafts", "sessions"),
    ] {
        g.open_scenario(Component::SessionBrowser, name).unwrap();
        let text = screen(&mut g, 110, 38);
        assert!(
            text.contains(expected),
            "{name}: missing {expected}\n{text}"
        );
    }
    for name in [
        "tiny",
        "narrow-40",
        "ansi16",
        "no-color",
        "ascii",
        "light",
        "truecolor-dark",
        "ansi256",
        "no-color-ascii",
    ] {
        g.open_scenario(Component::SessionBrowser, name).unwrap();
        assert!(!screen(&mut g, 40, 12).contains('\x1b'));
    }
}
#[test]
fn session_snapshots_delayed_results_picker_return_and_mid_draft_caret_are_retained() {
    let mut g = Gallery::new();
    for name in [
        "main",
        "search",
        "rename-refused",
        "delete-confirm",
        "delete-armed",
        "narrow",
        "ascii",
        "all-results",
    ] {
        g.open_scenario(Component::SessionBrowser, name).unwrap();
        insta::assert_snapshot!(format!("session_{name}"), screen(&mut g, 100, 28));
    }
    g.open_scenario(Component::SessionBrowser, "delayed")
        .unwrap();
    for _ in 0..24 {
        g.tick();
    }
    assert!(screen(&mut g, 100, 28).contains("forking"));
    g.tick();
    assert!(screen(&mut g, 100, 28).contains("forked"));
    g.open_scenario(Component::SessionBrowser, "rename")
        .unwrap();
    g.update(&Event::Paste("retained title".into()));
    g.update(&key(KeyCode::F(10)));
    assert_eq!(g.current(), "scenarios");
    g.update(&key(KeyCode::F(10)));
    assert!(screen(&mut g, 100, 28).contains("retained title"));
    g.update(&key(KeyCode::Esc));
    g.update(&key(KeyCode::Esc));
    assert!(screen(&mut g, 100, 28).contains("and then open the PR"));
    g.update(&key(KeyCode::Char('!')));
    assert!(screen(&mut g, 100, 28).contains("and then open !the PR"));
    g.update(&Event::Key(KeyEvent::new(
        KeyCode::Char('z'),
        KeyModifiers::CONTROL,
    )));
    assert!(screen(&mut g, 100, 28).contains("and then open the PR"));
    g.update(&Event::Key(KeyEvent::new(
        KeyCode::Char('x'),
        KeyModifiers::CONTROL,
    )));
    g.update(&key(KeyCode::Char('o')));
    assert!(screen(&mut g, 100, 28).contains("sessions"));
}
