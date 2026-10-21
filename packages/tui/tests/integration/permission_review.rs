//! Permission safety, lifecycle races, complete inspection and mixed review ownership.

use canonic_tui::{
    chat::{Anchor, Chat, Role, Turn},
    event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers},
    permission_review::{
        Choice, Intent, Kind, PermissionReview, Request, Resource, Response, Result as Reply, State,
    },
    question_form::{self, Question, QuestionForm},
    ratatui::{Terminal, backend::TestBackend},
    review_queue::{self, Review, ReviewQueue},
    theme::{ColorMode, Presentation, Symbols, Theme},
    tool_call_card::Output,
};
use std::time::Duration;

fn request() -> Request {
    let mut once = Choice::new("once", "Allow once", Kind::Allow);
    once.scope = "edit · settings.ts · once".into();
    once.detail = "this edit".into();
    let mut session = Choice::new("session", "Allow session", Kind::Allow);
    session.scope = "edit · src/digest/** · until you quit\nOnly these exact resources".into();
    session.wider = true;
    Request {
        id: "permission".into(),
        revision: 1,
        run: "run #276".into(),
        title: "edit files".into(),
        summary: "Edit settings.ts".into(),
        reason: "Timezone support".into(),
        resources: vec![Resource {
            name: "settings.ts".into(),
            detail: "+2 -1".into(),
        }],
        details_label: "diff".into(),
        details: Output::Diff("@@ -1,2 +1,2 @@\n-zone\n+timezone 東京\n context".into()),
        choices: vec![
            once,
            session,
            Choice::new("deny", "Deny", Kind::Deny),
            Choice::new("skip", "Skip", Kind::Skip),
        ],
        default: None,
    }
}
fn key(code: KeyCode) -> Event {
    Event::Key(KeyEvent::new(code, KeyModifiers::NONE))
}
fn press(review: &mut PermissionReview, code: KeyCode) {
    review.update(&key(code));
}
fn response(review: &mut PermissionReview) -> Response {
    let intents = review.take_intents();
    assert_eq!(intents.len(), 1);
    let Intent::Respond(response) = intents.into_iter().next().expect("valid test fixture") else {
        panic!("expected response")
    };
    response
}
fn sending() -> (PermissionReview, Response) {
    let mut r = PermissionReview::new(request()).expect("valid test fixture");
    press(&mut r, KeyCode::Char('1'));
    press(&mut r, KeyCode::Enter);
    let response = response(&mut r);
    (r, response)
}
fn draw(r: &mut PermissionReview, w: u16, h: u16, p: Presentation) -> String {
    let mut terminal = Terminal::new(TestBackend::new(w, h)).expect("valid test fixture");
    terminal
        .draw(|f| {
            r.render_with(f, f.area(), p, 0);
            r.render_overlay(f, f.area(), p);
        })
        .expect("valid test fixture");
    super::components::dump(terminal.backend().buffer())
}
#[test]
fn no_affirmative_default_and_highlighting_never_confirms() {
    let mut r = request();
    r.default = Some("once".into());
    assert!(PermissionReview::new(r).is_err());
    let mut r = request();
    r.default = Some("deny".into());
    let mut safe = PermissionReview::new(r).expect("valid test fixture");
    assert_eq!(
        safe.selected().expect("valid test fixture").kind,
        Kind::Deny
    );
    press(&mut safe, KeyCode::Enter);
    let sent = response(&mut safe);
    assert_eq!(sent.choice, "deny");
    let mut review = PermissionReview::new(request()).expect("valid test fixture");
    assert!(review.selected().is_none());
    press(&mut review, KeyCode::Enter);
    assert_eq!(review.take_intents().len(), 0);
    press(&mut review, KeyCode::Char('1'));
    assert_eq!(review.selected().expect("valid test fixture").id, "once");
    assert_eq!(review.take_intents().len(), 0);
    review.update(&Event::Paste("2\r\nAllow".into()));
    assert_eq!(review.selected().expect("valid test fixture").id, "once");
    assert!(review.notice().contains("paste ignored"));
    press(&mut review, KeyCode::Enter);
    let sent = response(&mut review);
    assert_eq!(
        (sent.id.as_str(), sent.revision, sent.choice.as_str()),
        ("permission", 1, "once")
    );
    assert_eq!(sent.scope, request().choices[0].scope);
    press(&mut review, KeyCode::Enter);
    press(&mut review, KeyCode::Esc);
    assert_eq!(review.state(), State::Sending);
    assert_eq!(review.take_intents().len(), 0);
    assert!(review.acknowledge(&sent.ticket, Reply::Applied));
    assert_eq!(review.state(), State::Allowed);
    assert!(!review.acknowledge(&sent.ticket, Reply::Applied));
}
#[test]
fn only_unmodified_enter_press_qualifies_every_confirmation_scope() {
    for code in [KeyCode::Enter, KeyCode::Char('r'), KeyCode::Esc] {
        for kind in [KeyEventKind::Repeat, KeyEventKind::Release] {
            let mut r = PermissionReview::new(request()).expect("valid test fixture");
            press(&mut r, KeyCode::Char('1'));
            r.update(&Event::Key(KeyEvent {
                code,
                modifiers: KeyModifiers::NONE,
                kind,
                state: canonic_tui::event::KeyEventState::NONE,
            }));
            assert_eq!(r.take_intents().len(), 0);
            assert!(!r.closed());
        }
    }
    let mut r = PermissionReview::new(request()).expect("valid test fixture");
    press(&mut r, KeyCode::Char('1'));
    r.update(&Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::ALT,
    )));
    assert_eq!(r.take_intents().len(), 0);
    press(&mut r, KeyCode::Char('2'));
    press(&mut r, KeyCode::Enter);
    press(&mut r, KeyCode::Char('2'));
    r.update(&Event::Key(KeyEvent {
        code: KeyCode::Enter,
        modifiers: KeyModifiers::NONE,
        kind: KeyEventKind::Repeat,
        state: canonic_tui::event::KeyEventState::NONE,
    }));
    assert_eq!(r.take_intents().len(), 0);
    assert!(r.confirming());
}
#[test]
fn scope_check_starts_back_and_preserves_exact_wording_and_choice() {
    let mut r = PermissionReview::new(request()).expect("valid test fixture");
    press(&mut r, KeyCode::Char('2'));
    press(&mut r, KeyCode::Enter);
    assert!(r.confirming());
    press(&mut r, KeyCode::Enter);
    assert!(!r.confirming());
    assert_eq!(r.selected().expect("valid test fixture").id, "session");
    assert_eq!(r.take_intents().len(), 0);
    press(&mut r, KeyCode::Enter);
    r.update(&Event::Paste("2\r".into()));
    assert!(r.notice().contains("paste ignored"));
    press(&mut r, KeyCode::Esc);
    assert!(!r.closed());
    press(&mut r, KeyCode::Enter);
    let text = draw(&mut r, 40, 18, Presentation::default());
    assert!(text.contains("Only these exact resources"));
    press(&mut r, KeyCode::Char('2'));
    assert_eq!(r.take_intents().len(), 0);
    press(&mut r, KeyCode::Enter);
    let sent = response(&mut r);
    assert_eq!(sent.scope, request().choices[1].scope);
}
#[test]
fn destructive_confirmation_disarms_on_movement_details_time_and_paste() {
    let mut descriptor = request();
    descriptor.choices[0].destructive = true;
    for interrupt in [
        key(KeyCode::Down),
        key(KeyCode::Tab),
        Event::Paste("\r".into()),
    ] {
        let mut r = PermissionReview::new(descriptor.clone()).expect("valid test fixture");
        press(&mut r, KeyCode::Char('1'));
        press(&mut r, KeyCode::Enter);
        assert_eq!(r.take_intents().len(), 0);
        r.update(&interrupt);
        press(&mut r, KeyCode::Char('1'));
        press(&mut r, KeyCode::Enter);
        assert_eq!(r.take_intents().len(), 0);
    }
    let mut r = PermissionReview::new(descriptor).expect("valid test fixture");
    press(&mut r, KeyCode::Char('1'));
    press(&mut r, KeyCode::Enter);
    r.tick(Duration::from_secs(3));
    press(&mut r, KeyCode::Enter);
    assert_eq!(r.take_intents().len(), 0);
    press(&mut r, KeyCode::Enter);
    assert_eq!(response(&mut r).choice, "once");
    let mut descriptor = request();
    descriptor.choices[1].destructive = true;
    let mut r = PermissionReview::new(descriptor).expect("valid test fixture");
    press(&mut r, KeyCode::Char('2'));
    press(&mut r, KeyCode::Enter);
    press(&mut r, KeyCode::Down);
    press(&mut r, KeyCode::Enter);
    assert_eq!(r.take_intents().len(), 0);
    press(&mut r, KeyCode::Enter);
    assert_eq!(response(&mut r).choice, "session");
}
#[test]
fn failure_never_retries_on_enter_and_explicit_r_uses_fresh_same_material_attempt() {
    let (mut r, first) = sending();
    assert!(r.acknowledge(&first.ticket, Reply::Failed("locked".into())));
    press(&mut r, KeyCode::Enter);
    assert_eq!(r.take_intents().len(), 0);
    press(&mut r, KeyCode::Tab);
    press(&mut r, KeyCode::Esc);
    press(&mut r, KeyCode::Enter);
    assert_eq!(r.take_intents().len(), 0);
    press(&mut r, KeyCode::Char('r'));
    let second = response(&mut r);
    assert_eq!(
        (&first.id, first.revision, &first.choice, &first.scope),
        (&second.id, second.revision, &second.choice, &second.scope)
    );
    assert_eq!(second.retry, 1);
    assert_ne!(first.ticket, second.ticket);
    assert!(!r.acknowledge(&first.ticket, Reply::Applied));
    assert!(r.acknowledge(&second.ticket, Reply::Failed("still locked".into())));
    press(&mut r, KeyCode::Char('3'));
    press(&mut r, KeyCode::Enter);
    assert_eq!(response(&mut r).choice, "deny");
}
#[test]
fn refusal_disables_only_attempted_choice_and_receipts_keep_negative_outcomes_distinct() {
    let (mut r, first) = sending();
    assert!(r.acknowledge(&first.ticket, Reply::Refused("not permitted".into())));
    assert!(r.selected().is_none());
    assert_eq!(r.state(), State::Refused);
    press(&mut r, KeyCode::Char('1'));
    press(&mut r, KeyCode::Enter);
    assert_eq!(r.take_intents().len(), 0);
    for (number, state) in [('3', State::Denied), ('4', State::Skipped)] {
        let mut r = PermissionReview::new(request()).expect("valid test fixture");
        press(&mut r, KeyCode::Char(number));
        press(&mut r, KeyCode::Enter);
        let sent = response(&mut r);
        r.acknowledge(&sent.ticket, Reply::Applied);
        assert_eq!(r.state(), state);
        assert!(r.closed());
    }
    let mut r = PermissionReview::new(request()).expect("valid test fixture");
    press(&mut r, KeyCode::Esc);
    assert_eq!(r.state(), State::Dismissed);
    assert!(matches!(
        r.take_intents().as_slice(),
        [Intent::Dismissed { .. }]
    ));
    assert!(r.receipt().is_none());
}
#[test]
fn replacement_expiry_withdrawal_and_cross_instance_tickets_cannot_resurrect_old_work() {
    let (mut r, first) = sending();
    let (_, foreign) = sending();
    assert!(!r.acknowledge(&foreign.ticket, Reply::Applied));
    let mut replacement = request();
    replacement.revision = 2;
    replacement.default = Some("deny".into());
    r.replace(replacement.clone(), "scope changed")
        .expect("valid test fixture");
    assert!(r.selected().is_none());
    assert!(!r.acknowledge(&first.ticket, Reply::Applied));
    assert!(r.replace(replacement, "bad revision").is_err());
    press(&mut r, KeyCode::Char('1'));
    press(&mut r, KeyCode::Enter);
    let second = response(&mut r);
    r.expire("stopped waiting");
    assert!(!r.acknowledge(&second.ticket, Reply::Applied));
    press(&mut r, KeyCode::Enter);
    assert_eq!(r.take_intents().len(), 0);
    press(&mut r, KeyCode::Tab);
    assert!(r.inspecting());
    press(&mut r, KeyCode::Esc);
    assert!(!r.closed());
    press(&mut r, KeyCode::Esc);
    assert!(r.closed());
    assert_eq!(r.state(), State::Expired);
    let (mut r, sent) = sending();
    assert!(r.acknowledge(&sent.ticket, Reply::Withdrawn("no longer needed".into())));
    assert_eq!(r.state(), State::Withdrawn);
    assert!(!r.closed());
    press(&mut r, KeyCode::Esc);
    assert!(r.closed());
}
#[test]
fn search_uses_full_loaded_source_and_enter_paste_escape_are_isolated() {
    let mut r = PermissionReview::new(request()).expect("valid test fixture");
    draw(&mut r, 40, 16, Presentation::default());
    r.update(&Event::Key(KeyEvent::new(
        KeyCode::Char('f'),
        KeyModifiers::CONTROL,
    )));
    for c in "timezone 東京".chars() {
        press(&mut r, KeyCode::Char(c));
    }
    r.update(&Event::Paste("3\r".into()));
    assert_eq!(r.query(), "timezone 東京");
    press(&mut r, KeyCode::Enter);
    assert_eq!(r.take_intents().len(), 0);
    assert!(draw(&mut r, 40, 16, Presentation::default()).contains("timezone"));
    press(&mut r, KeyCode::Esc);
    assert!(r.inspecting());
    press(&mut r, KeyCode::Esc);
    assert!(!r.inspecting());
    assert!(!r.closed());
    press(&mut r, KeyCode::Esc);
    assert!(r.closed());
}
#[test]
fn descriptors_are_atomic_bounded_and_never_shorten_scope_or_details() {
    let mut bad = request();
    bad.choices[1].scope = "unsafe\x1b scope".into();
    assert!(PermissionReview::new(bad).is_err());
    let mut bad = request();
    bad.choices[1].scope = "x".repeat(4097);
    assert!(PermissionReview::new(bad).is_err());
    let mut bad = request();
    bad.details = Output::Text("x".repeat(262_145));
    assert!(PermissionReview::new(bad).is_err());
    let mut bad = request();
    bad.choices[1].id = "once".into();
    assert!(PermissionReview::new(bad).is_err());
    let mut bad = request();
    bad.choices[1].kind = Kind::Deny;
    assert!(PermissionReview::new(bad).is_err());
    let mut bad = request();
    bad.reason = "x".repeat(65_537);
    assert!(PermissionReview::new(bad).is_err());
    let mut r = PermissionReview::new(request()).expect("valid test fixture");
    press(&mut r, KeyCode::Char('2'));
    press(&mut r, KeyCode::Enter);
    let mut bad = request();
    bad.revision = 2;
    bad.choices.clear();
    assert!(r.replace(bad, "invalid").is_err());
    assert!(r.confirming());
    assert_eq!(r.request().revision, 1);
    let mut descriptor = request();
    descriptor.choices = (0..32)
        .map(|i| Choice::new(&format!("deny-{i}"), &format!("Deny {i}"), Kind::Deny))
        .collect();
    let mut r = PermissionReview::new(descriptor).expect("valid test fixture");
    press(&mut r, KeyCode::End);
    assert!(draw(&mut r, 40, 12, Presentation::default()).contains("Deny 31"));
    press(&mut r, KeyCode::Enter);
    assert_eq!(response(&mut r).choice, "deny-31");
}
fn questions() -> Review {
    Review::Questions(Box::new(
        QuestionForm::new(question_form::Request {
            id: "questions".into(),
            revision: 1,
            title: "3 questions".into(),
            questions: vec![Question::new(
                "note",
                "note",
                "Note?",
                question_form::Kind::Text,
            )],
            partial: false,
            back: false,
        })
        .expect("valid test fixture"),
    ))
}
#[test]
fn mixed_queue_keeps_active_host_order_and_holds_active_withdrawal_until_close() {
    let mut q = ReviewQueue::default();
    q.note_typing();
    q.push(Review::Permission(Box::new(
        PermissionReview::new(request()).expect("valid test fixture"),
    )))
    .expect("valid test fixture");
    q.push(questions()).expect("valid test fixture");
    assert!(q.active().is_none());
    q.tick(Duration::from_millis(999));
    assert!(q.active().is_none());
    q.update(&Event::Key(KeyEvent::new(
        KeyCode::Char('g'),
        KeyModifiers::CONTROL,
    )));
    assert_eq!(q.active().expect("valid test fixture").id(), "permission");
    q.reorder(&["questions", "permission"])
        .expect("valid test fixture");
    assert_eq!(q.active().expect("valid test fixture").id(), "permission");
    assert_eq!(q.waiting_count(), 1);
    assert!(q.next_label().contains("3 questions"));
    assert!(q.reorder(&["permission", "permission"]).is_err());
    q.update(&key(KeyCode::Char('1')));
    q.update(&key(KeyCode::Enter));
    let intents = q.take_intents();
    let review_queue::Intent::Permission(Intent::Respond(sent)) = &intents[0] else {
        panic!("wrong review")
    };
    q.withdraw("permission");
    assert_eq!(q.active().expect("valid test fixture").id(), "permission");
    if let Some(Review::Permission(r)) = q.active_mut() {
        assert!(!r.acknowledge(&sent.ticket, Reply::Applied));
    }
    q.update(&key(KeyCode::Esc));
    assert_eq!(q.active().expect("valid test fixture").id(), "questions");
    assert!(matches!(
        q.take_intents().as_slice(),
        [review_queue::Intent::Permission(Intent::Dismissed { .. })]
    ));
}
#[test]
fn queue_prunes_undrained_old_attempts_after_material_change_and_withdrawal() {
    let mut q = ReviewQueue::default();
    q.push(Review::Permission(Box::new(
        PermissionReview::new(request()).expect("valid test fixture"),
    )))
    .expect("valid test fixture");
    q.update(&key(KeyCode::Char('1')));
    q.update(&key(KeyCode::Enter));
    if let Some(Review::Permission(r)) = q.active_mut() {
        let mut replacement = request();
        replacement.revision = 2;
        r.replace(replacement, "changed")
            .expect("valid test fixture");
    }
    assert_eq!(q.take_intents().len(), 0);
    q.update(&key(KeyCode::Char('1')));
    q.update(&key(KeyCode::Enter));
    q.withdraw("permission");
    assert_eq!(q.take_intents().len(), 0);
    q.push(questions()).expect("valid test fixture");
    let Some(Review::Questions(mut removed)) = q.withdraw("questions") else {
        panic!("removed questions")
    };
    removed.update(&Event::Paste("retained".into()));
    assert_eq!(removed.state(), question_form::State::Expired);
}
#[test]
fn mixed_queue_capacity_and_duplicate_identity_errors_leave_active_and_order_intact() {
    let mut queue = ReviewQueue::default();
    queue.push(questions()).unwrap();
    for index in 0..16 {
        let mut descriptor = request();
        descriptor.id = format!("permission-{index}");
        queue
            .push(Review::Permission(Box::new(
                PermissionReview::new(descriptor).unwrap(),
            )))
            .unwrap();
    }
    assert_eq!(queue.waiting_count(), 16);
    assert_eq!(queue.active().unwrap().id(), "questions");
    assert!(
        queue
            .push(Review::Permission(Box::new(
                PermissionReview::new(request()).unwrap()
            )))
            .is_err()
    );
    let mut duplicate = request();
    duplicate.id = "questions".into();
    assert!(
        queue
            .push(Review::Permission(Box::new(
                PermissionReview::new(duplicate).unwrap()
            )))
            .is_err()
    );
    assert_eq!(queue.waiting_count(), 16);
    queue.withdraw("permission-0");
    assert_eq!(queue.waiting_count(), 15);
    assert!(queue.next_label().starts_with("next: edit files"));
}
#[test]
fn long_host_titles_cannot_hide_the_actual_review_receipt_outcome() {
    for (number, expected) in [('1', "allowed"), ('3', "denied"), ('4', "skipped")] {
        let mut descriptor = request();
        descriptor.title = "東京 operation ".repeat(200);
        descriptor.choices[0].label = "very long host response label ".repeat(100);
        let mut panel = PermissionReview::new(descriptor).unwrap();
        press(&mut panel, KeyCode::Char(number));
        press(&mut panel, KeyCode::Enter);
        let response = response(&mut panel);
        panel.acknowledge(&response.ticket, Reply::Applied);
        let mut chat = Chat::new();
        chat.push_review(Review::Permission(Box::new(panel)))
            .unwrap();
        let receipt = chat.turn("review-receipt-1").unwrap();
        assert!(receipt.label.contains(expected), "{}", receipt.label);
        assert!(receipt.label.contains("..."));
    }
    let mut descriptor = question_form::Request {
        id: "long-questions".into(),
        revision: 1,
        title: "long question title ".repeat(200),
        questions: vec![Question::new(
            "note",
            "note",
            "Optional?",
            question_form::Kind::Text,
        )],
        partial: false,
        back: false,
    };
    let mut form = QuestionForm::new(descriptor.clone()).unwrap();
    form.update(&key(KeyCode::Enter));
    let Some(question_form::Intent::Submit(response)) = form.take_intents().pop() else {
        panic!("expected question response");
    };
    form.acknowledge(&response.ticket, question_form::Result::Accepted);
    let mut chat = Chat::new();
    chat.push_review(Review::Questions(Box::new(form))).unwrap();
    assert!(
        chat.turn("review-receipt-1")
            .unwrap()
            .label
            .contains("answered")
    );
    descriptor.id = "cancelled-questions".into();
    let mut form = QuestionForm::new(descriptor).unwrap();
    form.update(&key(KeyCode::Esc));
    form.update(&key(KeyCode::Esc));
    chat.push_review(Review::Questions(Box::new(form))).unwrap();
    assert!(
        chat.turn("review-receipt-2")
            .unwrap()
            .label
            .contains("cancelled")
    );
}
#[test]
fn chat_preserves_exact_draft_mode_anchor_follow_and_unread_on_every_outcome() {
    let mut chat = Chat::new();
    chat.set_input("mid draft");
    chat.push_record(Turn::new(
        "host",
        Role::Assistant,
        "acme",
        "line one\nline two",
    ));
    chat.tick(Duration::from_secs(1));
    chat.update(&key(KeyCode::Left));
    chat.update(&Event::Key(KeyEvent::new(
        KeyCode::Left,
        KeyModifiers::SHIFT,
    )));
    let document = chat.composer().editor().document();
    let mode = chat.mode();
    chat.jump(Anchor {
        message: "host".into(),
        part: "body".into(),
        offset: 0,
    });
    let anchor = chat.viewport_anchor().cloned();
    let follow = chat.following();
    chat.push_review(Review::Permission(Box::new(
        PermissionReview::new(request()).expect("valid test fixture"),
    )))
    .expect("valid test fixture");
    chat.append("concurrent unread output");
    assert!(chat.unread_count() > 0);
    chat.update(&Event::Paste("1\r".into()));
    assert_eq!(chat.composer().editor().document(), document);
    chat.update(&key(KeyCode::Esc));
    assert_eq!(chat.mode(), mode);
    assert_eq!(chat.composer().editor().document(), document);
    assert_eq!(chat.viewport_anchor().cloned(), anchor);
    assert_eq!(chat.following(), follow);
    assert!(chat.unread_count() > 0);
    chat.update(&Event::Paste("!".into()));
    chat.update(&Event::Key(KeyEvent::new(
        KeyCode::Char('z'),
        KeyModifiers::CONTROL,
    )));
    assert_eq!(chat.composer().editor().document(), document);
}
#[test]
fn every_profile_small_area_and_full_resource_search_render_without_terminal_controls() {
    for colors in [
        ColorMode::TrueColor,
        ColorMode::Ansi256,
        ColorMode::Ansi16,
        ColorMode::NoColor,
    ] {
        for symbols in [Symbols::Unicode, Symbols::Ascii] {
            for theme in [Theme::Dark, Theme::Light] {
                let p = Presentation {
                    colors,
                    symbols,
                    theme,
                };
                let mut r = PermissionReview::new(request()).expect("valid test fixture");
                press(&mut r, KeyCode::Tab);
                for (w, h) in [(80, 20), (40, 12), (12, 3), (1, 1)] {
                    let text = draw(&mut r, w, h, p);
                    assert!(!text.contains('\x1b'));
                }
                press(&mut r, KeyCode::Char('2'));
                press(&mut r, KeyCode::Enter);
                for (w, h) in [(80, 20), (40, 12), (1, 1)] {
                    draw(&mut r, w, h, p);
                }
            }
        }
    }
    let mut descriptor = request();
    descriptor.resources = (0..128)
        .map(|i| Resource {
            name: format!("resource-{i}"),
            detail: "affected".into(),
        })
        .collect();
    let mut r = PermissionReview::new(descriptor).expect("valid test fixture");
    draw(&mut r, 40, 15, Presentation::default());
    r.update(&Event::Key(KeyEvent::new(
        KeyCode::Char('f'),
        KeyModifiers::CONTROL,
    )));
    for c in "resource-127".chars() {
        press(&mut r, KeyCode::Char(c));
    }
    assert!(draw(&mut r, 40, 15, Presentation::default()).contains("resource-127"));
    assert_eq!(r.take_intents().len(), 0);
}
