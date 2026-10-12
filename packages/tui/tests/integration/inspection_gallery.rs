//! Live design fixtures, retained caller state and inspected snapshot coverage.
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
fn inspection_design_states_are_live_and_profiles_are_reachable() {
    let mut g = Gallery::new();
    for (name, expected) in [
        ("main", "digestHeaders"),
        ("code", "TypeScript"),
        ("diff", "nextSend"),
        ("empty", "empty content"),
        ("loading", "loading"),
        ("load-error", "permission denied"),
        ("retrying", "retrying"),
        ("binary", "Binary file"),
        ("generated", "Generated file"),
        ("too-large", "host limit"),
        ("partial", "partial content"),
        ("deleted", "removed offset table"),
        ("renamed", "R from fmt.ts"),
        ("long", "zone_1"),
        ("folded", "14 unchanged lines"),
        ("unfolded", "line-4"),
        ("next-hunk", "hunk 2 of 2"),
        ("last-hunk", "last hunk"),
        ("go-to-line", "ln 19"),
        ("whole-file", "whole file"),
        ("wrapped", "Pacific/Kiritimati"),
        ("find", "find"),
        ("next-match", "match 2"),
        ("no-match", "no match"),
        ("smart-case", "find"),
        ("selection", "3 lines selected"),
        ("copied-code", "copied"),
        ("copied-patch", "copied"),
        ("copy-failed", "couldn't copy"),
        ("editor-pending", "waiting for the host"),
        ("editor-failed", "couldn't open"),
        ("stale-result", "rejected: true"),
        ("foreign-result", "rejected: true"),
        ("duplicate-result", "rejected: true"),
        ("from-chat", "digestHeaders"),
        ("return-chat", "and bump the app version"),
        ("from-tree", "files"),
        ("return-tree", "settings.ts"),
        ("opening", "waiting for the host"),
        ("refused", "no longer available"),
        ("late-result", "rejected: true"),
        ("reopen", "ln 19"),
        ("narrow", "settings.ts"),
        ("narrow-code", "settings.ts"),
        ("tiny", "esc"),
        ("ansi16", "digestHeaders"),
        ("no-color", "digestHeaders"),
        ("ascii", "digestHeaders"),
        ("light", "digestHeaders"),
        ("unicode", "İSTANBUL"),
        ("controls", "<1B>"),
    ] {
        g.open_scenario(Component::CodeDiffInspection, name)
            .unwrap();
        let text = screen(&mut g, 110, 36);
        assert!(
            text.contains(expected),
            "{name}: missing {expected}\n{text}"
        );
        assert!(!text.contains("design pending"));
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
        g.open_scenario(Component::CodeDiffInspection, name)
            .unwrap();
        assert!(!screen(&mut g, 40, 12).contains('\x1b'));
    }
}
#[test]
fn inspection_snapshots_and_chat_tree_return_keep_independent_state() {
    let mut g = Gallery::new();
    for name in [
        "main",
        "folded",
        "find",
        "selection",
        "copy-failed",
        "narrow",
        "narrow-code",
        "tiny",
        "ascii",
        "from-tree",
    ] {
        g.open_scenario(Component::CodeDiffInspection, name)
            .unwrap();
        insta::assert_snapshot!(format!("inspection_{name}"), screen(&mut g, 80, 26));
    }
    g.open_scenario(Component::CodeDiffInspection, "from-chat")
        .unwrap();
    assert!(screen(&mut g, 100, 30).contains("digestHeaders"));
    g.update(&key(KeyCode::Esc));
    let chat = screen(&mut g, 100, 30);
    assert!(chat.contains("and bump the app version"));
    assert!(chat.contains("o open"));
    g.update(&key(KeyCode::Esc));
    g.update(&key(KeyCode::Char('!')));
    assert!(screen(&mut g, 100, 30).contains("and bump the app versio!n"));
    g.open_scenario(Component::CodeDiffInspection, "from-tree")
        .unwrap();
    g.update(&key(KeyCode::Tab));
    assert!(screen(&mut g, 100, 30).contains("settings.ts"));
    g.update(&key(KeyCode::Enter));
    assert!(screen(&mut g, 100, 30).contains("digestHeaders"));
    g.open_scenario(Component::CodeDiffInspection, "find")
        .unwrap();
    g.update(&key(KeyCode::F(10)));
    g.update(&key(KeyCode::F(10)));
    assert!(screen(&mut g, 100, 30).contains("find"));
}
