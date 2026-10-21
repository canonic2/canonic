//! Public status contracts: stable identities, explicit clocks, exact effects and safe rendering.
use canonic_tui::{
    chat::{Chat, Intent as ChatIntent},
    event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers},
    ratatui::{Terminal, backend::TestBackend, layout::Rect, style::Color},
    status_panels::{
        Action, Entry, Intent, Invalid, Kind, Mode, Notice, Outcome, Request, Row, Section,
        Severity, Snapshot, State, StatusPanels, Target,
    },
    theme::{ColorMode, Presentation, Symbols, Theme},
};
use std::time::Duration;
fn key(code: KeyCode) -> Event {
    Event::Key(KeyEvent::new(code, KeyModifiers::NONE))
}
fn data() -> Snapshot {
    let mut s = Section::new("tasks", "To-dos");
    s.state = State::Ready;
    s.detail.clear();
    s.summary = "2".into();
    s.rows = vec![Row::new("a", "first"), Row::new("b", "second")];
    s.rows[0].action = Some(Action::new("inspect", "inspect"));
    let mut n = Notice::new("error", Severity::Error, "Reconnect required");
    n.action = Some(Action::new("retry", "retry"));
    n.detail = "host reason".into();
    n.expires = Some(Duration::from_secs(1));
    let mut info = Notice::new("info", Severity::Info, "Updated");
    info.expires = Some(Duration::from_secs(2));
    Snapshot {
        revision: 1,
        sections: vec![s],
        notices: vec![info, n],
    }
}
fn panels() -> StatusPanels {
    StatusPanels::new(data()).expect("valid status fixture or operation")
}
fn request(p: &mut StatusPanels) -> Request {
    let intents = p.take_intents();
    let [Intent::Request(r)] = intents.as_slice() else {
        panic!("one request, got {intents:?}")
    };
    r.clone()
}
fn render(panels: &mut StatusPanels, w: u16, h: u16, p: Presentation, compact: bool) -> String {
    let mut terminal =
        Terminal::new(TestBackend::new(w, h)).expect("valid status fixture or operation");
    terminal
        .draw(|f| {
            if panels.mode() == Mode::Notices {
                panels.render_notices(f, f.area(), p);
            } else {
                panels.render_sections(f, f.area(), p, compact);
            }
        })
        .expect("valid status fixture or operation");
    super::components::dump(terminal.backend().buffer())
}
#[test]
fn navigation_folds_and_compact_render_preserve_host_order_and_cursor() {
    let mut p = panels();
    p.open(Mode::Status);
    p.update(&key(KeyCode::Down));
    let selected = Target::Section {
        id: "tasks".into(),
        row: Some("a".into()),
    };
    assert_eq!(p.cursor(), Some(selected.clone()));
    render(&mut p, 32, 12, Presentation::default(), true);
    assert!(!p.folded("tasks"));
    assert_eq!(p.cursor(), Some(selected));
    p.update(&key(KeyCode::Left));
    assert!(p.folded("tasks"));
    assert_eq!(
        p.cursor(),
        Some(Target::Section {
            id: "tasks".into(),
            row: None
        })
    );
    p.update(&key(KeyCode::Right));
    p.update(&key(KeyCode::Down));
    let mut next = p.snapshot().clone();
    next.revision += 1;
    next.sections[0].revision += 1;
    next.sections[0].rows.reverse();
    p.replace(next).expect("valid status fixture or operation");
    assert_eq!(
        p.cursor(),
        Some(Target::Section {
            id: "tasks".into(),
            row: Some("a".into())
        })
    );
    let mut next = p.snapshot().clone();
    next.revision += 1;
    next.sections.clear();
    p.replace(next).expect("valid status fixture or operation");
    assert_eq!(p.cursor(), None);
}
#[test]
fn actions_are_press_only_once_and_paste_cannot_issue_work() {
    let mut p = panels();
    p.open(Mode::Status);
    p.update(&key(KeyCode::Down));
    for kind in [KeyEventKind::Repeat, KeyEventKind::Release] {
        let mut k = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
        k.kind = kind;
        p.update(&Event::Key(k));
    }
    p.update(&Event::Paste("\rretry\n".into()));
    p.update(&Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::ALT,
    )));
    assert_eq!(p.take_intents(), []);
    p.update(&key(KeyCode::Enter));
    let r = request(&mut p);
    p.update(&key(KeyCode::Enter));
    assert_eq!(p.take_intents(), []);
    assert!(p.pending(&r));
    assert!(
        p.complete(&r, Outcome::Done)
            .expect("valid status fixture or operation")
    );
    assert!(
        !p.complete(&r, Outcome::Done)
            .expect("valid status fixture or operation")
    );
}
#[test]
fn exact_acknowledgements_reject_foreign_altered_and_materially_stale_requests() {
    let mut p = panels();
    let mut other = panels();
    p.open(Mode::Notices);
    other.open(Mode::Notices);
    p.update(&key(KeyCode::Enter));
    other.update(&key(KeyCode::Enter));
    let r = request(&mut p);
    let foreign = request(&mut other);
    assert!(
        !p.complete(&foreign, Outcome::Done)
            .expect("valid status fixture or operation")
    );
    let mut changed = r.clone();
    changed.action = "different".into();
    assert!(
        !p.complete(&changed, Outcome::Done)
            .expect("valid status fixture or operation")
    );
    assert!(p.pending(&r));
    let mut next = p.snapshot().clone();
    next.revision += 1;
    next.notices[1].revision += 1;
    next.notices[1].label = "new failure".into();
    p.replace(next).expect("valid status fixture or operation");
    assert!(
        !p.complete(&r, Outcome::Done)
            .expect("valid status fixture or operation")
    );
    assert!(!p.pending(&r));
    assert_eq!(p.unresolved(), 1);
}
#[test]
fn invalid_snapshot_and_outcome_preserve_facts_pending_and_focus() {
    let mut p = panels();
    p.open(Mode::Notices);
    p.update(&key(KeyCode::Enter));
    let r = request(&mut p);
    let original = p.snapshot().clone();
    let cursor = p.cursor();
    let mut wrong = original.clone();
    wrong.revision += 1;
    wrong.notices[1].label = "material change without record revision".into();
    assert_eq!(p.replace(wrong.clone()), Err(Invalid::Revision));
    assert_eq!(
        p.complete(&r, Outcome::Applied(wrong)),
        Err(Invalid::Revision)
    );
    assert_eq!(
        p.complete(&r, Outcome::Failed("x".repeat(65537))),
        Err(Invalid::Limit)
    );
    assert!(p.pending(&r));
    assert_eq!(p.snapshot(), &original);
    assert_eq!(p.cursor(), cursor);
}
#[test]
fn expiry_is_monotonic_and_actionable_errors_stay_reachable_when_set_aside() {
    let mut p = panels();
    p.advance(Duration::from_secs(2));
    assert_eq!(p.notices().len(), 1);
    assert_eq!(p.unresolved(), 1);
    assert!(!p.advance(Duration::from_secs(1)));
    assert_eq!(p.notices().len(), 1);
    assert!(p.dismiss("error"));
    assert!(p.current_notice().is_none());
    assert_eq!(p.unresolved(), 1);
    p.open(Mode::Notices);
    p.update(&key(KeyCode::Enter));
    let r = request(&mut p);
    assert!(
        p.complete(&r, Outcome::Done)
            .expect("valid status fixture or operation")
    );
    assert_eq!(p.unresolved(), 0);
    assert_eq!(p.notices(), [] as [&Notice; 0]);
}
#[test]
fn pending_and_failed_info_action_do_not_expire_or_lose_retry_after_refresh() {
    let mut d = data();
    d.notices.truncate(1);
    d.notices[0].action = Some(Action::new("open", "open"));
    let mut p = StatusPanels::new(d).expect("valid status fixture or operation");
    p.open(Mode::Notices);
    p.update(&key(KeyCode::Enter));
    let r = request(&mut p);
    p.advance(Duration::from_secs(10));
    assert_eq!(p.notices().len(), 1);
    p.complete(&r, Outcome::Failed("browser closed".into()))
        .expect("valid status fixture or operation");
    p.advance(Duration::from_secs(20));
    assert_eq!(p.unresolved(), 1);
    assert_eq!(
        p.current_notice()
            .expect("valid status fixture or operation")
            .id,
        "info"
    );
    let mut next = p.snapshot().clone();
    next.revision += 1;
    p.replace(next).expect("valid status fixture or operation");
    assert_eq!(p.failure(&r.target), Some("browser closed"));
    p.dismiss("info");
    assert!(p.current_notice().is_none());
    assert_eq!(p.unresolved(), 1);
    p.update(&key(KeyCode::Enter));
    let retry = request(&mut p);
    assert_ne!(retry.ticket, r.ticket);
    p.complete(&retry, Outcome::Done)
        .expect("valid status fixture or operation");
    assert_eq!(p.notices(), [] as [&Notice; 0]);
}
#[test]
fn return_does_not_cancel_work_or_reopen_pane_on_late_acknowledgement() {
    let mut p = panels();
    p.open(Mode::Notices);
    p.update(&key(KeyCode::Enter));
    let r = request(&mut p);
    p.update(&key(KeyCode::Esc));
    assert_eq!(p.take_intents(), vec![Intent::Returned]);
    assert!(p.pending(&r));
    assert!(
        p.complete(&r, Outcome::Failed("late failure".into()))
            .expect("valid status fixture or operation")
    );
    assert_eq!(p.mode(), Mode::Closed);
    assert_eq!(p.unresolved(), 1);
}
#[test]
fn severity_and_recency_choose_attention_and_revised_error_clears_set_aside() {
    let mut d = data();
    let mut n = d.notices[1].clone();
    n.id = "newer".into();
    n.published = Duration::from_secs(4);
    d.notices.push(n);
    let mut p = StatusPanels::new(d).expect("valid status fixture or operation");
    assert_eq!(
        p.current_notice()
            .expect("valid status fixture or operation")
            .id,
        "newer"
    );
    p.dismiss("newer");
    assert_eq!(
        p.current_notice()
            .expect("valid status fixture or operation")
            .id,
        "error"
    );
    let mut next = p.snapshot().clone();
    next.revision += 1;
    next.notices[2].revision += 1;
    p.replace(next).expect("valid status fixture or operation");
    assert_eq!(
        p.current_notice()
            .expect("valid status fixture or operation")
            .id,
        "newer"
    );
}
#[test]
fn unknown_cost_is_distinct_from_zero_and_retry_requires_offered_error_action() {
    let mut d = data();
    d.sections[0].rows.clear();
    d.sections[0].state = State::Unknown;
    d.sections[0].kind = Kind::Cost;
    let mut p = StatusPanels::new(d).expect("valid status fixture or operation");
    assert!(p.summary(64).contains("not reported"));
    p.open(Mode::Status);
    p.update(&key(KeyCode::Char('r')));
    assert_eq!(p.take_intents(), []);
    let mut next = p.snapshot().clone();
    next.revision += 1;
    next.sections[0].revision += 1;
    next.sections[0].state = State::Error;
    next.sections[0].retry = Some(Action::new("retry", "retry"));
    p.replace(next).expect("valid status fixture or operation");
    p.update(&key(KeyCode::Char('r')));
    assert_eq!(request(&mut p).action, "retry");
}
#[test]
fn disabled_actions_expose_reason_and_never_request_execution() {
    let mut d = data();
    d.notices[1]
        .action
        .as_mut()
        .expect("valid status fixture or operation")
        .disabled = Some("host policy blocks reconnect".into());
    let mut p = StatusPanels::new(d).expect("valid status fixture or operation");
    p.open(Mode::Notices);
    p.update(&key(KeyCode::Enter));
    assert_eq!(p.take_intents(), []);
    assert!(
        render(&mut p, 64, 14, Presentation::default(), false)
            .contains("host policy blocks reconnect")
    );
}
#[test]
fn long_reason_details_can_be_read_and_profiles_preserve_semantics() {
    let mut d = data();
    d.notices[1].detail = format!("{}\nEND OF REASON", "long reason text ".repeat(30));
    let mut p = StatusPanels::new(d).expect("valid status fixture or operation");
    p.open(Mode::Notices);
    for _ in 0..50 {
        p.update(&Event::Key(KeyEvent::new(
            KeyCode::PageDown,
            KeyModifiers::ALT,
        )));
    }
    assert!(render(&mut p, 32, 12, Presentation::default(), false).contains("END OF REASON"));
    for colors in [
        ColorMode::NoColor,
        ColorMode::Ansi16,
        ColorMode::Ansi256,
        ColorMode::TrueColor,
    ] {
        for theme in [Theme::Dark, Theme::Light] {
            for symbols in [Symbols::Ascii, Symbols::Unicode] {
                let presentation = Presentation {
                    colors,
                    theme,
                    symbols,
                };
                let text = render(&mut p, 32, 12, presentation, false);
                assert!(text.contains("Reconnect"));
                assert!(text.contains("esc back"));
                assert!(!text.contains('\x1b'));
                if symbols == Symbols::Ascii {
                    assert!(text.is_ascii());
                }
            }
        }
    }
}
#[test]
fn rendering_clips_to_subrectangle_and_retains_loading_height() {
    let mut p = panels();
    p.open(Mode::Status);
    render(&mut p, 32, 12, Presentation::default(), false);
    let mut next = p.snapshot().clone();
    next.revision += 1;
    next.sections[0].revision += 1;
    next.sections[0].rows.clear();
    next.sections[0].state = State::Loading;
    next.sections[0].detail = "waiting".into();
    p.replace(next).expect("valid status fixture or operation");
    let mut terminal =
        Terminal::new(TestBackend::new(50, 18)).expect("valid status fixture or operation");
    terminal
        .draw(|f| {
            f.render_widget(
                canonic_tui::ratatui::widgets::Block::default()
                    .style(canonic_tui::ratatui::style::Style::default().bg(Color::Blue)),
                f.area(),
            );
            p.render_sections(f, Rect::new(4, 3, 32, 10), Presentation::default(), false);
        })
        .expect("valid status fixture or operation");
    for y in 0..18 {
        for x in 0..50 {
            if !(4..36).contains(&x) || !(3..13).contains(&y) {
                assert_eq!(terminal.backend().buffer()[(x, y)].bg, Color::Blue);
            }
        }
    }
    for w in [0, 1, 2, 12] {
        for h in [0, 1, 2, 3] {
            render(&mut p, w, h, Presentation::default(), false);
        }
    }
}
#[test]
fn chat_entry_is_opt_in_and_preserves_draft_caret_and_scroll() {
    let mut chat = Chat::new();
    chat.set_input("draft");
    chat.update(&key(KeyCode::Left));
    chat.set_status_available(true);
    for (suffix, entry) in [('i', Entry::Status), ('n', Entry::Notices)] {
        chat.update(&Event::Key(KeyEvent::new(
            KeyCode::Char('x'),
            KeyModifiers::CONTROL,
        )));
        chat.update(&key(KeyCode::Char(suffix)));
        assert!(
            matches!(chat.take_intents().as_slice(), [ChatIntent::Status { entry: e, .. }] if *e == entry)
        );
    }
    chat.update(&key(KeyCode::Char('!')));
    assert_eq!(chat.input(), "draf!t");
    chat.update(&Event::Key(KeyEvent::new(
        KeyCode::Char('g'),
        KeyModifiers::CONTROL,
    )));
    assert!(matches!(
        chat.take_intents().as_slice(),
        [ChatIntent::Status {
            entry: Entry::Attention,
            ..
        }]
    ));
    chat.set_status_available(false);
    chat.update(&Event::Key(KeyEvent::new(
        KeyCode::Char('g'),
        KeyModifiers::CONTROL,
    )));
    assert_eq!(chat.take_intents(), []);
}

#[test]
fn status_footer_names_actions_and_composer_overlays_keep_input_precedence() {
    let mut p = panels();
    p.open(Mode::Status);
    p.update(&key(KeyCode::Down));
    assert!(
        render(
            &mut p,
            32,
            12,
            Presentation {
                symbols: Symbols::Ascii,
                ..Presentation::default()
            },
            false
        )
        .contains("enter inspect")
    );
    let mut chat = Chat::new();
    chat.set_status_available(true);
    chat.set_input("draft");
    chat.composer_mut().open_history_search();
    chat.update(&Event::Key(KeyEvent::new(
        KeyCode::Char('g'),
        KeyModifiers::CONTROL,
    )));
    assert_eq!(chat.take_intents(), []);
    assert_eq!(chat.input(), "draft");
    assert!(chat.composer_mut().overlay_open());
}

#[test]
fn long_section_projects_a_tiny_viewport_and_can_scroll_to_the_final_reason() {
    let mut d = data();
    d.sections[0].rows.clear();
    d.sections[0].state = State::Error;
    d.sections[0].detail = format!("{}END", "line\n".repeat(12000));
    let mut p = StatusPanels::new(d).expect("bounded long section");
    p.open(Mode::Status);
    let first = render(&mut p, 12, 4, Presentation::default(), false);
    assert!(first.contains("line"));
    for _ in 0..3100 {
        p.update(&Event::Key(KeyEvent::new(
            KeyCode::PageDown,
            KeyModifiers::ALT,
        )));
    }
    let last = render(&mut p, 12, 4, Presentation::default(), false);
    assert!(last.contains("END"), "{last}");
}
proptest::proptest! {
    #[test]
    fn arbitrary_clock_steps_never_revive_expired_info(steps in proptest::collection::vec(0u64..20, 0..40)) {
        let mut p = panels(); let mut expired = false;
        for seconds in steps { p.advance(Duration::from_secs(seconds)); expired |= seconds >= 2; proptest::prop_assert_eq!(p.notices().iter().any(|n| n.id == "info"), !expired); proptest::prop_assert_eq!(p.unresolved(), 1); }
    }
}

#[test]
fn invalid_id_duplicates_state_and_bounds_are_rejected_before_acceptance() {
    let mut d = data();
    d.sections.push(d.sections[0].clone());
    assert_eq!(StatusPanels::new(d).err(), Some(Invalid::Identity));
    let mut d = data();
    d.notices[0].id = "bad\nidentity".into();
    assert_eq!(StatusPanels::new(d).err(), Some(Invalid::Identity));
    let mut d = data();
    d.sections[0].state = State::Loading;
    assert_eq!(StatusPanels::new(d).err(), Some(Invalid::State));
    let mut d = data();
    d.sections[0].bar = Some(11);
    assert_eq!(StatusPanels::new(d).err(), Some(Invalid::State));
    let mut d = data();
    d.notices[0].detail = "x".repeat(65537);
    assert_eq!(StatusPanels::new(d).err(), Some(Invalid::Limit));
}

#[test]
fn control_data_is_safe_and_loading_keeps_the_next_section_in_place() {
    let mut d = data();
    let mut s = Section::new("second", "Second");
    s.state = State::Empty;
    s.detail = "no items".into();
    d.sections.push(s);
    d.sections[0].rows[0].label = "safe\x1b[31m\x07".into();
    let mut p = StatusPanels::new(d).expect("valid supplied control text");
    p.open(Mode::Status);
    let before = render(&mut p, 64, 16, Presentation::default(), false);
    assert!(!before.contains('\x1b'));
    assert!(before.contains("<1B>"));
    let before_row = before
        .lines()
        .position(|l| l.contains("Second"))
        .expect("second section visible");
    let mut next = p.snapshot().clone();
    next.revision += 1;
    next.sections[0].revision += 1;
    next.sections[0].rows.clear();
    next.sections[0].state = State::Loading;
    next.sections[0].detail = "loading".into();
    p.replace(next).expect("newer loading state");
    let after = render(&mut p, 64, 16, Presentation::default(), false);
    assert_eq!(
        after.lines().position(|l| l.contains("Second")),
        Some(before_row)
    );
}

#[test]
fn chat_review_and_run_attention_keep_priority_over_notices() {
    use canonic_tui::{
        picker::Choice,
        question_form::{Kind as QuestionKind, Question, QuestionForm, Request as QuestionRequest},
        review_queue::Review,
        run_activity::{Snapshot as RunSnapshot, State as RunState},
    };
    let mut chat = Chat::new();
    chat.set_input("draft");
    chat.set_status_available(true);
    let mut question = Question::new("question", "Question", "Choose", QuestionKind::Single);
    question.choices = vec![Choice::new("one", "One")];
    let form = QuestionForm::new(QuestionRequest {
        id: "review".into(),
        revision: 1,
        title: "Review".into(),
        questions: vec![question],
        partial: false,
        back: false,
    })
    .expect("valid question");
    chat.push_review(Review::Questions(Box::new(form)))
        .expect("one review");
    chat.update(&Event::Key(KeyEvent::new(
        KeyCode::Char('g'),
        KeyModifiers::CONTROL,
    )));
    assert_eq!(chat.take_intents(), []);
    assert_eq!(chat.input(), "draft");
    chat.reviews_mut().withdraw("review");
    chat.tick(Duration::from_secs(1));
    let mut run = RunSnapshot::new("run");
    run.state = RunState::Running;
    run.options_available = true;
    assert!(chat.begin_run(run));
    chat.update(&Event::Key(KeyEvent::new(
        KeyCode::Char('g'),
        KeyModifiers::CONTROL,
    )));
    assert!(
        !chat
            .take_intents()
            .iter()
            .any(|i| matches!(i, ChatIntent::Status { .. }))
    );
    assert_eq!(chat.input(), "draft");
}
