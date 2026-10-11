use canonic_tui::{
    run_activity::{Connection, RunActivity, Snapshot, State},
    theme::Symbols,
};
use std::time::Duration;

fn running(id: &str) -> RunActivity {
    let mut snapshot = Snapshot::new(id);
    snapshot.state = State::Running;
    snapshot.cancel_available = true;
    snapshot.options_available = true;
    snapshot.elapsed = Duration::from_secs(14);
    RunActivity::new(snapshot).expect("valid fixture")
}

#[test]
fn compact_waiting_and_cancel_failure_preserve_available_action_keys() {
    let mut run = running("failed");
    let ticket = run.cancel().unwrap().ticket;
    assert!(run.cancel_failed(&ticket, &"very long host reason ".repeat(30)));
    let label = run.label(24, Symbols::Unicode);
    assert!(label.starts_with("esc retry · ctrl+g"), "{label}");
    assert!(
        run.label(80, Symbols::Unicode)
            .contains("very long host reason")
    );
    let mut snapshot = Snapshot::new("input");
    snapshot.state = State::Waiting {
        questions: 0,
        subject: "permission review".into(),
    };
    snapshot.options_available = true;
    let run = RunActivity::new(snapshot).unwrap();
    assert_eq!(run.label(40, Symbols::Ascii), "ctrl+g opens . ? input");
    assert!(!run.label(80, Symbols::Unicode).contains("0 questions"));
}

#[test]
fn cancellation_is_single_use_and_completion_wins_without_fabricated_outcomes() {
    let mut run = running("run");
    let intent = run.cancel().unwrap();
    assert!(run.cancelling());
    assert_eq!(run.snapshot().state, State::Running);
    run.tick(Duration::from_secs(2));
    assert!(run.cancel().is_none());
    assert!(
        run.label(80, Symbols::Unicode)
            .contains("already asked · 2s")
    );
    let mut next = run.snapshot().clone();
    next.revision += 1;
    next.elapsed = Duration::from_secs(16);
    assert!(run.replace(next.clone()));
    assert!(run.cancelling());
    next.revision += 1;
    next.state = State::Completed;
    assert!(run.replace(next.clone()));
    assert!(!run.cancelling());
    assert!(!run.cancel_failed(&intent.ticket, "late failure"));
    assert_eq!(run.label(80, Symbols::Unicode), "ready");
    next.revision += 1;
    next.state = State::Cancelled;
    assert!(!run.replace(next));
}

#[test]
fn failed_cancel_can_retry_but_foreign_and_previous_run_tickets_are_inert() {
    let mut run = running("first");
    let foreign = running("first").cancel().unwrap();
    let first = run.cancel().unwrap();
    assert!(!run.cancel_failed(&foreign.ticket, "wrong owner"));
    assert!(run.cancel_failed(&first.ticket, "host timeout"));
    assert!(!run.cancel_failed(&first.ticket, "duplicate"));
    assert_eq!(run.snapshot().state, State::Running);
    assert!(
        run.label(80, Symbols::Unicode)
            .contains("host timeout · esc try again · ctrl+g options")
    );
    let options = run.options().unwrap();
    assert!(run.options().is_none());
    assert!(run.acknowledge_options(&options.ticket));
    assert!(!run.acknowledge_options(&options.ticket));
    let retry = run.cancel().unwrap();
    assert_ne!(first.ticket, retry.ticket);
    let next = running("second").snapshot().clone();
    assert!(run.begin(next));
    assert!(!run.cancelling());
    assert!(!run.cancel_failed(&retry.ticket, "old run"));
    assert!(run.cancel().is_some());
}

#[test]
fn transport_unknown_freezes_run_and_never_changes_a_finished_outcome() {
    let mut run = running("run");
    run.set_connection(Connection::Offline {
        retry_in: Some(Duration::from_secs(4)),
    });
    run.tick(Duration::from_secs(50));
    assert_eq!(run.snapshot().elapsed, Duration::from_secs(14));
    assert!(run.cancel().is_none());
    assert_eq!(
        run.label(80, Symbols::Unicode),
        "! disconnected · run state unknown"
    );
    assert_eq!(
        run.connection_label(Symbols::Unicode),
        "○ offline · retry in 4s"
    );
    let mut completed = run.snapshot().clone();
    completed.revision += 1;
    completed.state = State::Completed;
    assert!(run.replace(completed));
    assert_eq!(run.label(80, Symbols::Unicode), "ready");
    assert_eq!(
        run.connection_label(Symbols::Ascii),
        "o offline . retry in 4s"
    );
}

#[test]
fn injected_motion_and_capabilities_keep_seconds_host_owned() {
    let mut run = running("run");
    let before = run.label(80, Symbols::Unicode);
    run.tick(Duration::from_millis(140));
    assert_ne!(before, run.label(80, Symbols::Unicode));
    assert!(run.label(40, Symbols::Unicode).ends_with("14s · esc stops"));
    run.set_reduced_motion(true);
    let static_label = run.label(80, Symbols::Unicode);
    run.tick(Duration::from_secs(1));
    assert_eq!(static_label, run.label(80, Symbols::Unicode));
    let mut next = run.snapshot().clone();
    next.revision += 1;
    next.state = State::Waiting {
        questions: 2,
        subject: "acme is waiting\x1b[2J".into(),
    };
    next.cancel_available = false;
    assert!(run.replace(next));
    assert!(run.cancel().is_none());
    assert!(run.label(40, Symbols::Unicode).starts_with("ctrl+g opens"));
    assert!(!run.label(80, Symbols::Unicode).contains('\x1b'));
    assert!(run.label(80, Symbols::Unicode).contains("2 questions"));
    let mut next = run.snapshot().clone();
    next.revision += 1;
    next.options_available = false;
    assert!(run.replace(next));
    assert!(!run.label(40, Symbols::Unicode).contains("ctrl+g"));
}

#[test]
fn chat_run_intents_keep_draft_cursor_and_queue_until_host_confirmation() {
    use canonic_tui::{
        chat::{Action, Chat, Intent},
        event::{Event, KeyCode, KeyEvent, KeyModifiers},
    };
    let mut chat = Chat::new();
    chat.set_input("keep this draft");
    chat.update(&Event::Key(KeyEvent::new(
        KeyCode::Left,
        KeyModifiers::NONE,
    )));
    let cursor = chat.composer().editor().cursor();
    assert!(chat.begin_run(running("run").snapshot().clone()));
    let escape = Event::Key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert_eq!(chat.update(&escape), Action::None);
    let intents = chat.take_intents();
    let Intent::Run(cancel) = &intents[0] else {
        panic!("identified cancellation");
    };
    assert_eq!(chat.update(&escape), Action::None);
    assert_eq!(chat.take_intents(), []);
    assert_eq!(chat.input(), "keep this draft");
    assert_eq!(chat.composer().editor().cursor(), cursor);
    assert!(chat.composer().is_busy());
    assert!(chat.fail_run_cancel(&cancel.ticket, "host did not confirm"));
    let ctrl_c = Event::Key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL));
    assert_eq!(chat.update(&ctrl_c), Action::None);
    assert!(matches!(chat.take_intents().as_slice(), [Intent::Run(_)]));
    assert_eq!(chat.input(), "keep this draft");
    assert_eq!(
        chat.update(&Event::Key(KeyEvent::new(
            KeyCode::Enter,
            KeyModifiers::NONE
        ))),
        Action::None
    );
    assert_eq!(chat.composer().queue().len(), 1);
    let mut done = chat.run_activity().unwrap().snapshot().clone();
    done.revision += 1;
    done.state = State::Completed;
    assert!(chat.update_run(done));
    assert!(!chat.run_activity().unwrap().cancelling());
}

#[test]
fn escape_clears_editor_selection_before_requesting_a_busy_run_cancel() {
    use canonic_tui::{
        chat::Chat,
        event::{Event, KeyCode, KeyEvent, KeyModifiers},
    };
    let mut chat = Chat::new();
    chat.set_input("keep selected draft");
    chat.update(&Event::Key(KeyEvent::new(
        KeyCode::Char('g'),
        KeyModifiers::CONTROL,
    )));
    assert!(chat.composer().editor().selection().is_some());
    chat.begin_run(running("run").snapshot().clone());
    let escape = Event::Key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    chat.update(&escape);
    assert_eq!(chat.composer().editor().selection(), None);
    assert_eq!(chat.take_intents(), []);
    assert!(chat.composer().is_busy());
    assert_eq!(chat.input(), "keep selected draft");
    chat.update(&escape);
    assert_eq!(chat.take_intents().len(), 1);
}
