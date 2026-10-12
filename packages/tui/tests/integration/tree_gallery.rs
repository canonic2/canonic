//! Every named tree state exercises live navigation or a deterministic host result.
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
fn tree_design_states_are_real_fixtures_and_each_profile_is_reachable() {
    let mut g = Gallery::new();
    for (name, expected) in [
        ("main", "schedule"),
        ("files", "schedule.ts"),
        ("tasks", "digest-schedule"),
        ("cursor-open", "settings.ts"),
        ("unfocused", "main view"),
        ("long-names", "rendering.test.ts"),
        ("statuses", "status-Deleted"),
        ("disabled", "unavailable"),
        ("deep", "level-17"),
        ("paging", "next page"),
        ("progress", "12/40"),
        ("outcomes", "Cancelled"),
        ("filter", "schedule.ts"),
        ("unloaded", "profile/"),
        ("loading", "loading"),
        ("loaded", "api.ts"),
        ("empty", "no children"),
        ("load-failed", "permission denied"),
        ("retrying", "attempt 2"),
        ("refreshed", "quiet-hours.ts"),
        ("removed", "surviving ancestor"),
        ("ancestor-removed", "surviving ancestor"),
        ("bad-data", "skipped 2"),
        ("root-loading", "listing changes"),
        ("root-failed", "git exited 128"),
        ("root-retry", "loading"),
        ("open-folder", "digest/"),
        ("first-child", "settings.ts"),
        ("leaf-parent", "digest/"),
        ("close-folder", "digest/"),
        ("expand-all", "loading"),
        ("scroll", "file-79.ts"),
        ("toggle", "digest/"),
        ("collapse-all", "src/"),
        ("filter-clear", "settings.ts"),
        ("opening", "opening"),
        ("opened", "host opened schedule"),
        ("open-failed", "permission denied"),
        ("selecting", "selecting"),
        ("actions", "stop this task"),
        ("stopping", "stopping"),
        ("action-done", "cancelled by host"),
        ("action-failed", "permission denied"),
        ("stale-result", "rejected: true"),
        ("foreign-result", "rejected: true"),
        ("duplicate-result", "rejected: true"),
        ("material-changed", "updated by host"),
        ("concurrent", "opening"),
        ("late-collapsed", "profile/"),
        ("no-ack", "opening"),
        ("delayed", "opening"),
        ("full-screen", "files"),
        ("panel", "Inspection fixture"),
        ("medium", "Inspection fixture"),
        ("narrow", "files"),
        ("tiny", "files"),
        ("before-chat", "and then inspect the change"),
        ("returning", "and then inspect the change"),
        ("palette", "Files tree"),
        ("narrow-filter", "schedule"),
        ("long-error", "host permission explanation"),
        ("paste-isolated", "schedule"),
        ("ascii", "schedule.ts"),
        ("no-color", "schedule.ts"),
        ("light", "schedule.ts"),
        ("reduced-motion", "selecting"),
    ] {
        g.open_scenario(Component::TreeBrowser, name).unwrap();
        let text = screen(&mut g, 110, 38);
        assert!(
            text.contains(expected),
            "{name}: missing {expected}\n{text}"
        );
    }
    for name in [
        "narrow-40",
        "ansi16",
        "no-color",
        "ascii",
        "light",
        "truecolor-dark",
        "ansi256",
        "no-color-ascii",
    ] {
        g.open_scenario(Component::TreeBrowser, name).unwrap();
        assert!(!screen(&mut g, 40, 12).contains('\x1b'));
    }
}
#[test]
fn tree_snapshots_picker_retention_focus_and_chat_entry_preserve_draft() {
    let mut g = Gallery::new();
    for name in [
        "files",
        "tasks",
        "filter",
        "load-failed",
        "actions",
        "deep",
        "narrow",
        "ascii",
    ] {
        g.open_scenario(Component::TreeBrowser, name).unwrap();
        insta::assert_snapshot!(format!("tree_{name}"), screen(&mut g, 100, 28));
    }
    g.open_scenario(Component::TreeBrowser, "filter").unwrap();
    g.update(&Event::Paste("ed".into()));
    g.update(&key(KeyCode::F(10)));
    g.update(&key(KeyCode::F(10)));
    assert!(screen(&mut g, 100, 28).contains("sched"));
    g.update(&key(KeyCode::Esc));
    g.update(&key(KeyCode::Esc));
    assert!(screen(&mut g, 100, 28).contains("and then inspect the change"));
    g.update(&key(KeyCode::Char('!')));
    assert!(screen(&mut g, 100, 28).contains("and then inspect the !change"));
    g.update(&Event::Key(KeyEvent::new(
        KeyCode::Char('z'),
        KeyModifiers::CONTROL,
    )));
    assert!(screen(&mut g, 100, 28).contains("and then inspect the change"));
    g.update(&Event::Key(KeyEvent::new(
        KeyCode::Char('x'),
        KeyModifiers::CONTROL,
    )));
    g.update(&key(KeyCode::Char('f')));
    assert!(screen(&mut g, 100, 28).contains("files"));
    g.open_scenario(Component::TreeBrowser, "panel").unwrap();
    g.update(&key(KeyCode::Tab));
    assert!(screen(&mut g, 100, 28).contains("main view focused"));
    g.update(&key(KeyCode::Tab));
    g.update(&key(KeyCode::Enter));
    g.update(&key(KeyCode::F(5)));
    assert!(screen(&mut g, 100, 28).contains("host opened schedule"));
    g.open_scenario(Component::TreeBrowser, "opened").unwrap();
    g.update(&key(KeyCode::Esc));
    let returned = screen(&mut g, 100, 28);
    assert!(
        returned.contains("and then inspect the change")
            && returned.contains("Inspection fixture line 2")
    );
    assert!(!returned.contains("Inspection fixture for schedule"));
    g.open_scenario(Component::TreeBrowser, "delayed").unwrap();
    for _ in 0..24 {
        g.tick();
    }
    assert!(screen(&mut g, 100, 28).contains("opening"));
    g.tick();
    assert!(screen(&mut g, 100, 28).contains("host opened schedule"));
    g.open_scenario(Component::TreeBrowser, "no-ack").unwrap();
    for _ in 0..100 {
        g.tick();
    }
    assert!(screen(&mut g, 100, 28).contains("opening"));
    g.open_scenario(Component::TreeBrowser, "main").unwrap();
    g.update(&key(KeyCode::Char('/')));
    g.update(&Event::Paste("schedule".into()));
    let mut terminal = Terminal::new(TestBackend::new(100, 28)).unwrap();
    terminal.draw(|f| g.render(f)).unwrap();
    assert!(
        terminal.get_cursor_position().unwrap().x < 34,
        "the focused tree filter owns the cursor"
    );
}
