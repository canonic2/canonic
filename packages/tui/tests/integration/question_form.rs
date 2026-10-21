//! Exercises identified review lifecycles, isolated editing and responsive public rendering.

use canonic_tui::{
    chat::{Chat, Role, Turn},
    editor::{Editor, Outcome},
    event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers},
    picker::Choice,
    question_form::{
        AnswerValue, Intent, Kind, Question, QuestionForm, Request, Response, Result as Reply,
        State,
    },
    ratatui::{Terminal, backend::TestBackend},
    review_queue::{Review, ReviewQueue},
    theme::{ColorMode, Presentation, Symbols},
};
use std::time::Duration;

fn key(c: KeyCode) -> Event {
    Event::Key(KeyEvent::new(c, KeyModifiers::NONE))
}
fn press(f: &mut QuestionForm, c: KeyCode) {
    f.update(&key(c));
}
fn request() -> Request {
    let mut single = Question::new("audience", "audience", "Which audience?", Kind::Single);
    single.required = true;
    single.other = true;
    single.choices = vec![
        Choice::new("all", "Everyone"),
        Choice::new("eu", "EU users"),
    ];
    single.default = Some("eu".into());
    let mut multi = Question::new("channels", "channels", "Which channels?", Kind::Multiple);
    multi.required = true;
    multi.minimum = 1;
    multi.maximum = Some(2);
    multi.other = true;
    multi.choices = vec![
        Choice::new("email", "Email"),
        Choice::new("web", "Web"),
        Choice::new("sms", "SMS"),
    ];
    multi.choices[2].disabled = Some("not configured".into());
    let mut note = Question::new("note", "note", "Optional note?", Kind::Text);
    note.characters = Some(4);
    Request {
        id: "form".into(),
        revision: 1,
        title: "Request".into(),
        questions: vec![single, multi, note],
        partial: false,
        back: false,
    }
}
fn response(f: &mut QuestionForm) -> Response {
    f.take_intents()
        .into_iter()
        .find_map(|i| {
            if let Intent::Submit(r) = i {
                Some(r)
            } else {
                None
            }
        })
        .expect("one submit intent")
}
fn review(f: &mut QuestionForm) {
    press(f, KeyCode::Char('1'));
    press(f, KeyCode::Down);
    press(f, KeyCode::Char(' '));
    press(f, KeyCode::Up);
    press(f, KeyCode::Char(' '));
    press(f, KeyCode::Enter);
    press(f, KeyCode::Tab);
}
#[test]
fn single_confirmation_and_multi_review_have_distinct_boundaries() {
    let mut r = request();
    r.questions.truncate(1);
    let mut one = QuestionForm::new(r).unwrap();
    assert_eq!(one.answers().len(), 0);
    one.update(&Event::Paste("1\n".into()));
    assert_eq!(one.take_intents().len(), 0);
    let mut repeat = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
    repeat.kind = KeyEventKind::Repeat;
    one.update(&Event::Key(repeat));
    assert_eq!(one.take_intents().len(), 0);
    press(&mut one, KeyCode::Enter);
    let sent = response(&mut one);
    assert_eq!(
        sent.answers[0].value,
        AnswerValue::Choices {
            ids: vec!["eu".into()],
            other: None
        }
    );
    press(&mut one, KeyCode::Enter);
    assert_eq!(one.take_intents().len(), 0);
    assert_eq!(one.state(), State::Submitting);
    assert!(one.acknowledge(&sent.ticket, Reply::Accepted));
    assert!(!one.acknowledge(&sent.ticket, Reply::Accepted));
    let mut many = QuestionForm::new(request()).unwrap();
    review(&mut many);
    assert_eq!(many.take_intents().len(), 0);
    press(&mut many, KeyCode::Enter);
    let sent = response(&mut many);
    assert_eq!(sent.answers.len(), 2);
    assert_eq!(
        sent.answers[1].value,
        AnswerValue::Choices {
            ids: vec!["email".into(), "web".into()],
            other: None
        }
    );
}
#[test]
fn accepted_and_cancelled_lifetimes_cannot_be_reopened_by_replacement() {
    let mut accepted = QuestionForm::new(request()).unwrap();
    review(&mut accepted);
    press(&mut accepted, KeyCode::Enter);
    let sent = response(&mut accepted);
    assert!(accepted.acknowledge(&sent.ticket, Reply::Accepted));
    let receipt = accepted.receipt().cloned();
    let mut replacement = request();
    replacement.revision += 1;
    assert!(accepted.replace(replacement.clone()).is_err());
    assert_eq!(accepted.receipt(), receipt.as_ref());
    let mut cancelled = QuestionForm::new(request()).unwrap();
    press(&mut cancelled, KeyCode::Esc);
    assert_eq!(cancelled.state(), State::Cancelled);
    assert!(cancelled.replace(replacement).is_err());
}
#[test]
fn validation_navigation_and_refusal_preserve_every_staged_answer() {
    let mut f = QuestionForm::new(request()).unwrap();
    press(&mut f, KeyCode::Tab);
    press(&mut f, KeyCode::Enter);
    assert_eq!(f.notice(), "pick at least one");
    press(&mut f, KeyCode::End);
    press(&mut f, KeyCode::Up);
    press(&mut f, KeyCode::Char(' '));
    assert_eq!(f.notice(), "not configured");
    press(&mut f, KeyCode::Home);
    press(&mut f, KeyCode::Char(' '));
    press(&mut f, KeyCode::Enter);
    f.update(&Event::Paste("é東京ab".into()));
    press(&mut f, KeyCode::Enter);
    assert!(f.notice().contains("5 of 4"));
    assert_eq!(f.field(2).unwrap().text(), "é東京ab");
    press(&mut f, KeyCode::Tab);
    press(&mut f, KeyCode::Enter);
    assert_eq!(f.step(), 0);
    assert_eq!(f.take_intents().len(), 0);
    press(&mut f, KeyCode::Char('1'));
    assert_eq!(f.step(), 3);
    press(&mut f, KeyCode::Char('3'));
    press(&mut f, KeyCode::Backspace);
    press(&mut f, KeyCode::Enter);
    assert_eq!(f.step(), 3);
    press(&mut f, KeyCode::Enter);
    let sent = response(&mut f);
    let staged = f.answers();
    assert!(f.acknowledge(
        &sent.ticket,
        Reply::Refused {
            question: "channels".into(),
            reason: "unavailable on this plan".into()
        }
    ));
    assert_eq!(f.step(), 1);
    assert_eq!(f.answers(), staged);
    assert!(f.notice().contains("unavailable"));
}
#[test]
fn fields_retain_over_limit_text_and_atomic_resource_rejection_preserves_selection_undo() {
    let mut e = Editor::new(5);
    e.set_reject_overflow(true);
    e.insert("abc");
    e.update(&Event::Key(KeyEvent::new(
        KeyCode::Char('g'),
        KeyModifiers::CONTROL,
    )));
    let selected = e.selection();
    let cursor = e.cursor();
    assert_eq!(e.insert("abcdef"), Outcome::Rejected);
    assert_eq!(e.text(), "abc");
    assert_eq!(e.selection(), selected);
    assert_eq!(e.cursor(), cursor);
    e.update(&Event::Key(KeyEvent::new(
        KeyCode::Char('z'),
        KeyModifiers::CONTROL,
    )));
    assert_eq!(e.text(), "");
    let mut f = QuestionForm::new(request()).unwrap();
    press(&mut f, KeyCode::End);
    f.update(&Event::Paste("typed\nelse".into()));
    press(&mut f, KeyCode::Esc);
    let a = f.answers();
    press(&mut f, KeyCode::Delete);
    assert_eq!(f.answers().len(), 0);
    f.update(&Event::Key(KeyEvent::new(
        KeyCode::Char('z'),
        KeyModifiers::CONTROL,
    )));
    assert_eq!(f.answers(), a);
    f.update(&Event::Paste("x".repeat(65_537)));
    assert_eq!(f.field(0).unwrap().text(), "typed\nelse");
    assert!(f.notice().contains("insertion rejected"));
}
#[test]
fn cancellation_defaults_keep_and_partial_requires_an_explicit_offer_and_action() {
    for partial in [false, true] {
        let mut r = request();
        r.partial = partial;
        let mut f = QuestionForm::new(r).unwrap();
        press(&mut f, KeyCode::Char('1'));
        assert_eq!(f.step(), 1);
        press(&mut f, KeyCode::Esc);
        assert_eq!(f.step(), 0, "a later question goes back one step");
        assert!(!f.cancelling());
        assert_eq!(f.answers().len(), 1, "going back keeps answers");
        press(&mut f, KeyCode::Esc);
        assert!(f.cancelling(), "the first question asks to cancel");
        press(&mut f, KeyCode::Enter);
        assert!(!f.cancelling());
        assert_eq!(f.take_intents().len(), 0);
        press(&mut f, KeyCode::Esc);
        press(&mut f, KeyCode::Esc);
        assert!(!f.cancelling());
        press(&mut f, KeyCode::Esc);
        f.update(&Event::Paste("2\n".into()));
        assert_eq!(f.take_intents().len(), 0);
        press(&mut f, KeyCode::Char('2'));
        if partial {
            let sent = response(&mut f);
            assert!(sent.partial);
            assert_eq!(sent.answers.len(), 1);
        } else {
            assert_eq!(f.state(), State::Cancelled);
            assert!(matches!(
                f.take_intents().as_slice(),
                [Intent::Cancelled { .. }]
            ));
        }
    }
}
#[test]
fn stale_foreign_expired_and_duplicate_acknowledgements_never_accept_new_requests() {
    let mut f = QuestionForm::new(request()).unwrap();
    review(&mut f);
    press(&mut f, KeyCode::Enter);
    let old = response(&mut f);
    let mut foreign = QuestionForm::new(request()).unwrap();
    review(&mut foreign);
    press(&mut foreign, KeyCode::Enter);
    let foreign = response(&mut foreign);
    assert!(!f.acknowledge(&foreign.ticket, Reply::Accepted));
    assert_eq!(f.state(), State::Submitting);
    assert!(f.acknowledge(&old.ticket, Reply::Failed("offline".into())));
    let answers = f.answers();
    press(&mut f, KeyCode::Enter);
    let retry = response(&mut f);
    assert_ne!(old.ticket, retry.ticket);
    assert_eq!(f.answers(), answers);
    let mut replacement = request();
    replacement.revision = 2;
    f.replace(replacement).unwrap();
    assert_eq!(f.answers().len(), 0);
    assert!(!f.acknowledge(&retry.ticket, Reply::Accepted));
    press(&mut f, KeyCode::Char('y'));
    let copy = f.take_intents();
    let Intent::Copy { ticket, text, .. } = &copy[0] else {
        panic!("copy intent")
    };
    assert!(text.contains("Everyone"));
    assert!(f.acknowledge_copy(ticket, Err("clipboard unavailable")));
    assert!(!f.acknowledge_copy(ticket, Ok(())));
    f.expire("host deadline");
    press(&mut f, KeyCode::Char('1'));
    assert_eq!(f.take_intents().len(), 0);
    assert_eq!(f.state(), State::Expired);
    press(&mut f, KeyCode::Esc);
    assert!(f.closed());
}
#[test]
fn invalid_requests_and_replacement_are_atomic_and_review_reaches_question_thirty_two() {
    let mut r = request();
    r.questions[1].maximum = Some(0);
    r.questions[1].minimum = 1;
    assert!(QuestionForm::new(r).is_err());
    let mut f = QuestionForm::new(request()).unwrap();
    press(&mut f, KeyCode::Char('1'));
    let a = f.answers();
    let mut bad = request();
    bad.revision = 2;
    bad.questions[1].id = "audience".into();
    assert!(f.replace(bad).is_err());
    assert_eq!(f.answers(), a);
    let mut r = request();
    r.questions = (0..32)
        .map(|i| Question::new(&format!("q{i}"), "note", "Optional?", Kind::Text))
        .collect();
    let mut f = QuestionForm::new(r).unwrap();
    for _ in 0..32 {
        press(&mut f, KeyCode::Tab);
    }
    press(&mut f, KeyCode::End);
    press(&mut f, KeyCode::Enter);
    assert_eq!(f.step(), 31);
}
#[test]
fn host_ordered_queue_defers_typing_preserves_active_and_rejects_bad_orders() {
    let mut q = ReviewQueue::default();
    q.note_typing();
    q.push(Review::Questions(Box::new(
        QuestionForm::new(request()).unwrap(),
    )))
    .unwrap();
    assert!(q.active().is_none());
    q.tick(Duration::from_millis(900));
    q.note_typing();
    q.tick(Duration::from_millis(900));
    assert!(q.active().is_none());
    assert!(q.update(&Event::Key(KeyEvent::new(
        KeyCode::Char('g'),
        KeyModifiers::CONTROL
    ))));
    assert_eq!(q.active().unwrap().id(), "form");
    let mut r = request();
    r.id = "later".into();
    q.push(Review::Questions(Box::new(QuestionForm::new(r).unwrap())))
        .unwrap();
    assert!(q.reorder(&["later", "form"]).is_ok());
    assert_eq!(q.active().unwrap().id(), "form");
    assert!(q.reorder(&["form", "form"]).is_err());
    q.withdraw("form");
    assert_eq!(q.active().unwrap().id(), "later");
    assert_eq!(q.take_intents().len(), 0);
}
#[test]
fn chat_restores_exact_document_cursor_selection_mode_and_undo_while_streams_stay_unread() {
    let mut chat = Chat::new();
    chat.set_input("and bump the version");
    chat.update(&key(KeyCode::Left));
    chat.update(&Event::Key(KeyEvent::new(
        KeyCode::Left,
        KeyModifiers::SHIFT,
    )));
    let document = chat.composer().editor().document();
    chat.push_record(Turn::new(
        "stream",
        Role::Assistant,
        "acme",
        "Long streamed text\n",
    ));
    chat.reviews_mut()
        .push(Review::Questions(Box::new(
            QuestionForm::new(request()).unwrap(),
        )))
        .unwrap();
    chat.update(&Event::Key(KeyEvent::new(
        KeyCode::Char('g'),
        KeyModifiers::CONTROL,
    )));
    chat.update(&key(KeyCode::Char('1')));
    chat.append("Unread incoming line\n");
    assert!(chat.unread_count() > 0);
    // Back to the first question, then its cancellation dialog.
    chat.update(&key(KeyCode::Esc));
    chat.update(&key(KeyCode::Esc));
    chat.update(&key(KeyCode::Char('2')));
    assert_eq!(chat.composer().editor().document(), document);
    assert!(chat.unread_count() > 0);
    chat.update(&Event::Paste("!".into()));
    assert_eq!(chat.input(), "and bump the versi!n");
    chat.update(&Event::Key(KeyEvent::new(
        KeyCode::Char('z'),
        KeyModifiers::CONTROL,
    )));
    assert_eq!(chat.composer().editor().document(), document);
}
#[test]
fn public_rendering_handles_small_rectangles_profiles_fields_and_dialogs() {
    for colors in [
        ColorMode::Ansi16,
        ColorMode::Ansi256,
        ColorMode::TrueColor,
        ColorMode::NoColor,
    ] {
        for symbols in [Symbols::Ascii, Symbols::Unicode] {
            let mut f = QuestionForm::new(request()).unwrap();
            review(&mut f);
            for (w, h) in [(80, 20), (40, 12), (12, 3), (1, 1)] {
                let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
                terminal
                    .draw(|frame| {
                        f.render_with(
                            frame,
                            frame.area(),
                            Presentation {
                                colors,
                                symbols,
                                ..Presentation::default()
                            },
                            1,
                        );
                    })
                    .unwrap();
                let text = super::components::dump(terminal.backend().buffer());
                if w == 80 {
                    assert!(text.contains("Everyone"));
                    assert!(text.contains("+1 waiting"));
                }
            }
            press(&mut f, KeyCode::Char('3'));
            f.update(&Event::Paste("é東京\n🙂".into()));
            let mut terminal = Terminal::new(TestBackend::new(40, 16)).unwrap();
            terminal
                .draw(|frame| {
                    f.render_with(
                        frame,
                        frame.area(),
                        Presentation {
                            colors,
                            symbols,
                            ..Presentation::default()
                        },
                        0,
                    );
                })
                .unwrap();
            let text = super::components::dump(terminal.backend().buffer());
            assert!(text.contains("é東 京"), "{text}");
        }
    }
}
fn labeled() -> Request {
    let mut q = Question::new("profile", "profile", "Restrict the profile?", Kind::Single);
    q.required = true;
    q.other = true;
    q.choices = vec![Choice::new("unset", "Any profile")];
    q.other_label = Some("Only this profile".into());
    q.placeholder = Some("acme-read-only".into());
    Request {
        id: "labeled".into(),
        revision: 1,
        title: "Setup".into(),
        questions: vec![q],
        partial: false,
        back: false,
    }
}
fn draw(f: &mut QuestionForm, width: u16, symbols: Symbols, colors: ColorMode) -> String {
    let mut terminal = Terminal::new(TestBackend::new(width, 10)).unwrap();
    terminal
        .draw(|frame| {
            f.render_with(
                frame,
                frame.area(),
                Presentation {
                    colors,
                    symbols,
                    ..Presentation::default()
                },
                0,
            );
        })
        .unwrap();
    super::components::dump(terminal.backend().buffer())
}
#[test]
fn labeled_typed_row_shows_its_label_and_never_submits_the_placeholder() {
    let mut f = QuestionForm::new(labeled()).unwrap();
    let text = draw(&mut f, 80, Symbols::Unicode, ColorMode::TrueColor);
    assert!(text.contains("Only this profile"), "{text}");
    assert!(text.contains("acme-read-only"), "{text}");
    assert!(!text.contains("Something else"), "{text}");
    for width in [40, 80] {
        let text = draw(&mut f, width, Symbols::Ascii, ColorMode::NoColor);
        assert!(text.contains("Only this profile"), "{text}");
    }
    press(&mut f, KeyCode::Down);
    press(&mut f, KeyCode::Enter);
    let text = draw(&mut f, 80, Symbols::Unicode, ColorMode::TrueColor);
    assert!(
        text.contains("acme-read-only"),
        "empty field shows placeholder: {text}"
    );
    press(&mut f, KeyCode::Enter);
    assert!(
        f.take_intents()
            .iter()
            .all(|i| !matches!(i, Intent::Submit(_))),
        "an empty typed row submits nothing"
    );
    for c in "ops".chars() {
        press(&mut f, KeyCode::Char(c));
    }
    press(&mut f, KeyCode::Enter);
    let sent = response(&mut f);
    assert_eq!(
        sent.answers[0].value,
        AnswerValue::Choices {
            ids: vec![],
            other: Some("ops".into())
        }
    );
}
#[test]
fn typed_row_label_and_placeholder_are_validated() {
    let mut bad = labeled();
    bad.questions[0].other = false;
    bad.questions[0].choices.push(Choice::new("b", "B"));
    assert!(QuestionForm::new(bad).is_err(), "label without a typed row");
    let mut long = labeled();
    long.questions[0].other_label = Some("x".repeat(65));
    assert!(QuestionForm::new(long).is_err());
    let mut text = labeled();
    text.questions[0] = Question::new("note", "note", "Note?", Kind::Text);
    text.questions[0].placeholder = Some("an example".into());
    assert!(
        QuestionForm::new(text).is_ok(),
        "text questions take a placeholder"
    );
}
#[test]
fn a_step_of_a_host_flow_goes_back_without_the_cancel_dialog() {
    let mut r = labeled();
    r.back = true;
    let mut f = QuestionForm::new(r).unwrap();
    assert!(f.hint().contains("esc back"), "{}", f.hint());
    press(&mut f, KeyCode::Down);
    press(&mut f, KeyCode::Enter);
    for c in "ops".chars() {
        press(&mut f, KeyCode::Char(c));
    }
    press(&mut f, KeyCode::Esc);
    press(&mut f, KeyCode::Esc);
    assert!(
        !f.cancelling(),
        "typed text does not open the cancel dialog"
    );
    assert_eq!(f.state(), State::Cancelled);
    assert!(matches!(
        f.take_intents().as_slice(),
        [Intent::Cancelled { .. }]
    ));
    let first = QuestionForm::new(labeled()).unwrap();
    assert!(first.hint().contains("esc cancel"), "{}", first.hint());
}
