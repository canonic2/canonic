//! Live Question Form fixtures and focus-preserving gallery navigation.

use canonic_tui::{
    event::{Event, KeyCode, KeyEvent, KeyModifiers},
    gallery::{Component, Gallery},
    ratatui::{Terminal, backend::TestBackend},
};
fn key(c: KeyCode) -> Event {
    Event::Key(KeyEvent::new(c, KeyModifiers::NONE))
}
fn screen(g: &mut Gallery, w: u16, h: u16) -> String {
    let mut t = Terminal::new(TestBackend::new(w, h)).unwrap();
    t.draw(|f| g.render(f)).unwrap();
    super::components::dump(t.backend().buffer())
}
#[test]
fn question_gallery_projects_live_answer_and_request_states() {
    let mut g = Gallery::new();
    for (name, expected) in [
        ("main", "Who should get"),
        ("single", "Who should get"),
        ("multiple", "2 of 3 marked"),
        ("limit-reached", "unmark one first"),
        ("disabled", "needs a migration"),
        ("required-missing", "pick at least one"),
        ("typed", "Digest times"),
        ("something-else", "Only EU users"),
        ("review", "digest emails, weekly report"),
        ("review-missing", "! required"),
        ("back", "Who should get"),
        ("skip-required", "Anything to mention"),
        ("review-edit", "from review"),
        ("empty-field", "type here"),
        ("multiline", "timezone. Users"),
        ("other-field", "Only EU users"),
        ("other-cleared", "answer cleared"),
        ("other-undo", "Only EU users"),
        ("too-long", "214 of 200"),
        ("submitting", "sending answers"),
        ("accepted", "answered"),
        ("failed", "server rejected"),
        ("refused", "isn't available"),
        ("cancel-with-answers", "Keep answering"),
        ("cancelled", "nothing sent"),
        ("partial-offered", "Send answers so far"),
        ("partial-submitting", "sending answers"),
        ("partial-accepted", "partial"),
        ("expired", "expired"),
        ("replaced", "earlier questions"),
        ("waiting-open", "ctrl+g opens"),
        ("queue", "+1 waiting"),
        ("no-ack", "keys paused"),
        ("delayed-ack", "sending answers"),
        ("foreign-ack", "foreign acknowledgement rejected: true"),
        ("stale-ack", "stale acknowledgement rejected: true"),
        ("streaming", "digest rollout"),
        ("draft-restored", "answered"),
        ("long-choices", "more"),
        ("long-prompt", "Choose the audience"),
        ("resource-limit", "insertion rejected"),
        ("typed-required", "type an answer"),
        ("single-typed", "profile timezone"),
        ("narrow", "Anything to mention"),
        ("narrow-review", "digest emails"),
        ("narrow-field", "Digest times"),
        ("reduced-motion", "Digest times"),
        ("copy-failed", "clipboard unavailable"),
        ("expired-copy", "answers copied"),
        ("queue-reordered", "+1 waiting"),
        ("queue-withdrawn", "Anything to mention"),
    ] {
        g.open_scenario(Component::QuestionForm, name).unwrap();
        let text = screen(&mut g, 110, 42);
        assert!(text.contains(expected), "{name}:\n{text}");
    }
}
#[test]
fn question_gallery_snapshots_and_delayed_host_result_are_reviewable() {
    let mut g = Gallery::new();
    for name in [
        "main",
        "review",
        "expired",
        "cancel-with-answers",
        "narrow-review",
    ] {
        g.open_scenario(Component::QuestionForm, name).unwrap();
        insta::assert_snapshot!(format!("question_{name}"), screen(&mut g, 80, 28));
    }
    g.open_scenario(Component::QuestionForm, "delayed-ack")
        .unwrap();
    for _ in 0..26 {
        g.tick();
    }
    assert!(screen(&mut g, 100, 30).contains("answered"));
    g.open_scenario(Component::QuestionForm, "waiting-open")
        .unwrap();
    g.update(&Event::Key(KeyEvent::new(
        KeyCode::Char('g'),
        KeyModifiers::CONTROL,
    )));
    assert!(!screen(&mut g, 100, 30).contains("ctrl+g opens"));
    g.update(&key(KeyCode::F(10)));
    g.update(&key(KeyCode::F(10)));
    assert_eq!(g.current(), "question-form");
}
