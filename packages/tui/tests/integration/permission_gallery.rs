//! Live permission fixture meanings, reviewed profiles and queue acknowledgements.

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
fn permission_gallery_frames_exercise_real_public_transitions_and_host_results() {
    let mut g = Gallery::new();
    for (name, expected) in [
        ("main", "nothing chosen"),
        ("safe-default", "enter Deny"),
        ("details", "timezone"),
        ("chosen", "nothing allowed yet"),
        ("paste-ignored", "paste ignored"),
        ("search", "zone"),
        ("scope-check", "Exact host scope"),
        ("scope-back", "scope not confirmed"),
        ("scope-sending", "sending"),
        ("allowed-session", "allowed"),
        ("covered-later", "allowed this session"),
        ("waiting-open", "ctrl+g opens"),
        ("mixed-queue", "+2 waiting"),
        ("next-request", "run a command"),
        ("many-resources", "more resources"),
        ("command", "npm run migrate:digest"),
        ("network", "status.example.com"),
        ("destructive", "Delete these 3 files"),
        ("armed", "armed"),
        ("disarmed", "nothing allowed yet"),
        ("arm-expired", "nothing allowed yet"),
        ("sending", "sending"),
        ("allowed", "allowed"),
        ("failed", "locked by a build"),
        ("retry", "retry 1"),
        ("no-ack", "14s"),
        ("delayed-ack", "sending"),
        ("foreign-ack", "foreign acknowledgement rejected: true"),
        ("stale-ack", "stale acknowledgement rejected: true"),
        ("changed", "choice was cleared"),
        ("changed-scope", "edits in src/"),
        ("expired", "expired"),
        ("expired-sending", "nothing was allowed"),
        ("withdrawn", "withdrawn"),
        ("dismissed", "dismissed"),
        ("denied", "denied"),
        ("skipped", "skipped"),
        ("refused", "session grants are off"),
        ("queue-reordered", "next: digest rollout"),
        ("queue-withdrawn", "+1 waiting"),
        ("streaming", "nothing chosen"),
        ("draft-restored", "and bump the app version"),
        ("anchor-restored", "nothing chosen"),
        ("linked-card", "answer below"),
        ("card-resumed", "applied"),
        ("narrow", "this edit"),
        ("narrow-details", "timezone"),
        ("narrow-scope", "src/digest/**"),
        ("short", "details"),
        ("long-scope", "more"),
        ("long-details", "loaded inspection"),
        ("long-choices", "Deny response"),
        ("disabled", "session grants are off"),
        ("reduced-motion", "nothing chosen"),
        ("scope-destructive", "Exact host scope"),
    ] {
        g.open_scenario(Component::PermissionReview, name).unwrap();
        let text = screen(&mut g, 100, 38);
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
        g.open_scenario(Component::PermissionReview, name).unwrap();
        assert!(!screen(&mut g, 40, 12).contains('\x1b'));
    }
}
#[test]
fn permission_snapshots_ack_delay_inspection_scope_and_gallery_return_are_reviewable() {
    let mut g = Gallery::new();
    for name in [
        "main",
        "details",
        "scope-check",
        "failed",
        "refused",
        "narrow",
        "narrow-scope",
    ] {
        g.open_scenario(Component::PermissionReview, name).unwrap();
        insta::assert_snapshot!(format!("permission_{name}"), screen(&mut g, 80, 28));
    }
    g.open_scenario(Component::PermissionReview, "delayed-ack")
        .unwrap();
    for _ in 0..24 {
        g.tick();
    }
    assert!(screen(&mut g, 80, 28).contains("sending"));
    g.tick();
    assert!(screen(&mut g, 80, 28).contains("allowed"));
    g.open_scenario(Component::PermissionReview, "main")
        .unwrap();
    g.update(&key(KeyCode::Char('2')));
    g.update(&key(KeyCode::Enter));
    assert!(screen(&mut g, 80, 28).contains("Exact host scope"));
    g.update(&key(KeyCode::F(10)));
    assert_eq!(g.current(), "scenarios");
    g.update(&Event::Paste("narrow".into()));
    g.update(&key(KeyCode::F(10)));
    assert!(screen(&mut g, 80, 28).contains("Exact host scope"));
    g.update(&key(KeyCode::Esc));
    assert!(screen(&mut g, 80, 28).contains("scope not confirmed"));
    g.open_scenario(Component::PermissionReview, "waiting-open")
        .unwrap();
    g.update(&Event::Key(KeyEvent::new(
        KeyCode::Char('g'),
        KeyModifiers::CONTROL,
    )));
    assert!(screen(&mut g, 80, 28).contains("nothing chosen"));
}
