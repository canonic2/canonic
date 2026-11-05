//! Shared prompt keys, setup defaults, validation, rendering and native restoration.
use super::*;
use crate::{
    event::KeyEvent,
    ratatui::{Terminal, backend::TestBackend},
    theme::{ColorMode, Presentation},
};

#[cfg(unix)]
#[test]
#[ignore = "invoked in a disposable PTY by shared_prompt_terminal"]
fn prompt_child() {
    let choices = (0..12)
        .map(|i| Choice::new(i.to_string(), format!("Agent {i}")))
        .collect();
    let mut prompt = PickerPrompt::multiple(
        "Select agents",
        "Space marks agents; Enter continues.",
        choices,
        &[],
        1,
        "continue",
    )
    .unwrap()
    .back(true);
    assert_eq!(
        Host::open(16).unwrap().pick(&mut prompt).unwrap(),
        Outcome::Back
    );
    assert_eq!(
        Host::open(16).unwrap().pick(&mut prompt).unwrap(),
        Outcome::Submitted(vec!["9".into()])
    );
    println!("PROMPT_OK");
}

#[cfg(unix)]
#[test]
fn shared_prompt_terminal() {
    let output = std::process::Command::new("python3")
        .arg("-B")
        .arg(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/cli_prompts.py"
        ))
        .arg(std::env::current_exe().unwrap())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[cfg(unix)]
#[test]
#[ignore = "invoked in a disposable PTY by review_confirmation_expires"]
fn review_expiry_child() {
    let mut choice =
        permission_review::Choice::new("delete", "Delete Acme", permission_review::Kind::Allow);
    choice.destructive = true;
    let request = permission_review::Request {
        id: "acme-delete".into(),
        revision: 1,
        run: "Acme".into(),
        title: "Delete review".into(),
        summary: "Delete Acme".into(),
        reason: String::new(),
        resources: vec![],
        details_label: String::new(),
        details: crate::tool_call_card::Output::Text(String::new()),
        choices: vec![choice],
        default: None,
    };
    assert_eq!(
        Host::open(16).unwrap().review(request, 0).unwrap(),
        Outcome::Submitted("delete".into())
    );
    println!("REVIEW_OK");
}

#[cfg(unix)]
#[test]
fn review_confirmation_expires() {
    let output = std::process::Command::new("python3")
        .arg("-B")
        .arg(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/review_expiry.py"
        ))
        .arg(std::env::current_exe().unwrap())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
fn collected(picker: &mut ChoicePicker, event: &Event, available: bool) -> Option<Vec<String>> {
    match picker_event(picker, event, available) {
        Some(Outcome::Submitted(ids)) => Some(ids),
        _ => None,
    }
}
fn accepted(picker: &mut ChoicePicker, event: &Event, available: bool) -> Option<String> {
    collected(picker, event, available).and_then(|ids| ids.into_iter().next())
}
#[test]
fn numbered_question_answers_need_a_visible_confirmation_region() {
    let question = Question {
        title: "Acme mode".into(),
        prompt: "Choose a mode".into(),
        context: String::new(),
        choices: vec![("safe".into(), "Safe".into(), String::new())],
        multiple: false,
        typed: None,
        default: None,
        back: false,
    };
    let mut questions = setup_form(&question).unwrap();
    let number = Event::Key(KeyEvent::new(KeyCode::Char('1'), KeyModifiers::NONE));
    question_event(&mut questions, &number, false);
    assert!(questions.take_intents().is_empty());
    question_event(&mut questions, &number, true);
    let response = questions
        .take_intents()
        .into_iter()
        .find_map(|intent| {
            if let form::Intent::Submit(response) = intent {
                Some(response)
            } else {
                None
            }
        })
        .unwrap();
    assert_eq!(reply(&response), Reply::Choices(vec!["safe".into()]));
}

#[test]
fn mixed_choice_and_typed_questions_refuse_without_losing_selections() {
    let mut question = Question {
        title: "Acme targets".into(),
        prompt: "Choose targets".into(),
        context: String::new(),
        choices: vec![
            ("one".into(), "One".into(), String::new()),
            ("two".into(), "Two".into(), String::new()),
        ],
        multiple: true,
        typed: Some(("Another target".into(), None)),
        default: None,
        back: false,
    };
    assert_eq!(
        setup_form(&question).err().unwrap().kind(),
        io::ErrorKind::InvalidInput
    );
    question.typed = None;
    let mut form = setup_form(&question).unwrap();
    for code in [
        KeyCode::Char(' '),
        KeyCode::Down,
        KeyCode::Char(' '),
        KeyCode::Enter,
    ] {
        form.update(&Event::Key(KeyEvent::new(code, KeyModifiers::NONE)));
    }
    let response = form
        .take_intents()
        .into_iter()
        .find_map(|intent| match intent {
            form::Intent::Submit(response) => Some(response),
            _ => None,
        })
        .unwrap();
    assert_eq!(
        reply(&response),
        Reply::Choices(vec!["one".into(), "two".into()])
    );
    question.multiple = false;
    question.typed = Some(("Another target".into(), None));
    assert!(setup_form(&question).is_ok());
}

#[cfg(unix)]
#[test]
#[ignore = "invoked in a disposable PTY by question_clocks_follow_elapsed_time"]
fn question_clock_child() {
    let question = Question {
        title: "Acme input".into(),
        prompt: "Enter a target".into(),
        context: String::new(),
        choices: vec![],
        multiple: false,
        typed: None,
        default: None,
        back: false,
    };
    let mut form = setup_form(&question).unwrap();
    form.update(&Event::Paste("x".repeat(65_537)));
    assert!(form.field(0).unwrap().notice().is_some());
    assert_eq!(
        Host::open(16)
            .unwrap()
            .question_loop(&mut form, &mut |_| Ok(()))
            .unwrap(),
        Outcome::Back
    );
    assert!(
        form.field(0).unwrap().notice().is_none(),
        "editor notice must expire by elapsed time"
    );
    println!("CLOCK_OK");
}

#[cfg(unix)]
#[test]
fn question_clocks_follow_elapsed_time() {
    let output = std::process::Command::new("python3")
        .arg("-B")
        .arg(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/question_clock.py"
        ))
        .arg(std::env::current_exe().unwrap())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn question_submission_requires_visible_prompt_and_answer_rows() {
    let question = Question {
        title: "Acme mode".into(),
        prompt: "Choose a mode".into(),
        context: String::new(),
        choices: vec![("safe".into(), "Safe answer".into(), String::new())],
        multiple: false,
        typed: None,
        default: None,
        back: false,
    };
    for code in [KeyCode::Char('1'), KeyCode::Enter] {
        let mut questions = setup_form(&question).unwrap();
        for height in [3, 4, 5] {
            let mut terminal = Terminal::new(TestBackend::new(80, height)).unwrap();
            terminal
                .draw(|frame| render_question(&mut questions, frame, frame.area()))
                .unwrap();
            let text = terminal
                .backend()
                .buffer()
                .content()
                .iter()
                .map(crate::ratatui::buffer::Cell::symbol)
                .collect::<String>();
            let available = question_fits(&questions, terminal.get_frame().area());
            if height == 3 {
                assert!(!text.contains("Safe answer"), "{text}");
            }
            if available {
                assert!(text.contains("Choose a mode"), "{text}");
                assert!(text.contains("Safe answer"), "{text}");
            }
            question_event(
                &mut questions,
                &Event::Key(KeyEvent::new(code, KeyModifiers::NONE)),
                available,
            );
            assert_eq!(!questions.take_intents().is_empty(), height == 5);
        }
    }
}

#[cfg(unix)]
#[test]
#[ignore = "invoked in a disposable PTY by question_terminal_geometry"]
fn question_geometry_child() {
    let question = Question {
        title: "Acme mode".into(),
        prompt: "Choose a mode".into(),
        context: String::new(),
        choices: vec![
            ("safe".into(), "Safe answer".into(), String::new()),
            ("hidden".into(), "Hidden answer".into(), String::new()),
        ],
        multiple: false,
        typed: None,
        default: None,
        back: false,
    };
    assert_eq!(
        Host::open(16)
            .unwrap()
            .question(&question, &mut |_| Ok(()))
            .unwrap(),
        Outcome::Back
    );
    println!("QUESTION_OK");
}

#[cfg(unix)]
#[test]
fn question_terminal_geometry() {
    let output = std::process::Command::new("python3")
        .arg("-B")
        .arg(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/question_geometry.py"
        ))
        .arg(std::env::current_exe().unwrap())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
#[test]
fn common_setup_removal_starts_unmarked_and_confirmation_defaults_to_cancel() {
    let key = |code| Event::Key(KeyEvent::new(code, KeyModifiers::NONE));
    let mut removal =
        super::super::setup::agent_prompt(vec![Choice::new("acme", "Acme")], &["acme"], true)
            .unwrap();
    assert_eq!(removal.update(&key(KeyCode::Enter), true), None);
    removal.update(&key(KeyCode::Char(' ')), true);
    assert_eq!(
        removal.update(&key(KeyCode::Enter), true),
        Some(Outcome::Submitted(vec!["acme".into()]))
    );
    let mut confirm =
        super::super::setup::confirmation_prompt("Reviewed configuration paths").unwrap();
    assert_eq!(
        confirm.update(&key(KeyCode::Enter), true),
        Some(Outcome::Submitted(vec!["cancel".into()]))
    );
    let mut action = super::super::setup::action_prompt().unwrap();
    assert_eq!(
        action.update(&key(KeyCode::Enter), true),
        Some(Outcome::Submitted(vec!["install".into()]))
    );
}
#[test]
fn filtered_marks_survive_back_and_enter_below_minimum_stays_open() {
    let choices = (0..12)
        .map(|i| Choice::new(i.to_string(), format!("Agent {i}")))
        .collect();
    let mut prompt = PickerPrompt::multiple("Agents", "", choices, &[], 1, "continue")
        .unwrap()
        .back(true);
    let key = |code| Event::Key(KeyEvent::new(code, KeyModifiers::NONE));
    assert_eq!(prompt.update(&key(KeyCode::Enter), true), None);
    assert!(prompt.picker.notice().is_some());
    prompt.update(&key(KeyCode::Char('9')), true);
    assert_eq!(prompt.picker.query(), "9");
    prompt.update(&key(KeyCode::Char(' ')), true);
    assert_eq!(prompt.picker.query(), "9");
    assert_eq!(prompt.update(&key(KeyCode::Esc), true), None);
    assert!(prompt.picker.query().is_empty());
    assert_eq!(prompt.update(&key(KeyCode::Esc), true), Some(Outcome::Back));
    // Revisiting the same prompt retains marks even after a Back outcome.
    assert_eq!(
        prompt.update(&key(KeyCode::Enter), true),
        Some(Outcome::Submitted(vec!["9".into()]))
    );
    for code in [KeyCode::Char('c'), KeyCode::Char('d')] {
        assert_eq!(
            prompt.update(
                &Event::Key(KeyEvent::new(code, KeyModifiers::CONTROL)),
                true
            ),
            Some(Outcome::Cancelled)
        );
    }
}
#[test]
fn impossible_minimum_and_duplicate_ids_refuse_before_terminal_acquisition() {
    let mut disabled = Choice::new("disabled", "Unavailable");
    disabled.disabled = Some("not supported".into());
    assert_eq!(
        PickerPrompt::multiple("Agents", "", vec![disabled], &[], 1, "continue")
            .err()
            .unwrap()
            .kind(),
        io::ErrorKind::InvalidInput
    );
    assert!(
        PickerPrompt::single(
            "Agents",
            "",
            vec![Choice::new("same", "One"), Choice::new("same", "Two")],
            None
        )
        .is_err()
    );
}
#[test]
fn setup_questions_show_controls_and_preserve_refused_text() {
    let question = Question {
        title: "Acme profile".into(),
        prompt: "Restrict the profile?".into(),
        context: String::new(),
        choices: vec![("unset".into(), "Any profile".into(), String::new())],
        multiple: false,
        typed: Some(("Only this profile".into(), None)),
        default: None,
        back: false,
    };
    let mut questions = setup_form(&question).unwrap();
    let mut terminal = Terminal::new(TestBackend::new(120, 12)).unwrap();
    for expected in ["enter choose", "ctrl+j new line"] {
        terminal
            .draw(|frame| render_question(&mut questions, frame, frame.area()))
            .unwrap();
        let text = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(crate::ratatui::buffer::Cell::symbol)
            .collect::<String>();
        assert!(text.contains(expected), "{text}");
        if expected == "enter choose" {
            questions.update(&Event::Key(KeyEvent::new(
                KeyCode::Char('2'),
                KeyModifiers::NONE,
            )));
        }
    }
    for code in [
        KeyCode::Char('a'),
        KeyCode::Char('c'),
        KeyCode::Char('m'),
        KeyCode::Char('e'),
        KeyCode::Enter,
    ] {
        questions.update(&Event::Key(KeyEvent::new(code, KeyModifiers::NONE)));
    }
    let response = questions
        .take_intents()
        .into_iter()
        .find_map(|intent| {
            if let form::Intent::Submit(response) = intent {
                Some(response)
            } else {
                None
            }
        })
        .unwrap();
    assert_eq!(reply(&response), Reply::Typed("acme".into()));
    questions.acknowledge(
        &response.ticket,
        form::Result::Refused {
            question: "answer".into(),
            reason: "Choose a registered profile".into(),
        },
    );
    terminal
        .draw(|frame| render_question(&mut questions, frame, frame.area()))
        .unwrap();
    let text = terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(crate::ratatui::buffer::Cell::symbol)
        .collect::<String>();
    assert!(text.contains("acme"), "{text}");
    assert!(text.contains("Choose a registered profile"), "{text}");
}
#[test]
fn multiple_agents_are_staged_and_empty_confirmation_skips() {
    let mut picker = ChoicePicker::new(
        vec![
            Choice::new("claude", "Claude Code"),
            Choice::new("codex", "Codex"),
        ],
        None,
        SelectionMode::Multiple,
        Filter::Off,
    )
    .unwrap();
    let key = |code| Event::Key(KeyEvent::new(code, KeyModifiers::NONE));
    assert_eq!(
        collected(&mut picker, &key(KeyCode::Enter), true),
        Some(vec![])
    );
    assert_eq!(collected(&mut picker, &key(KeyCode::Char('1')), true), None);
    assert_eq!(collected(&mut picker, &key(KeyCode::Char('2')), true), None);
    assert_eq!(collected(&mut picker, &key(KeyCode::Enter), false), None);
    assert_eq!(
        collected(&mut picker, &key(KeyCode::Enter), true),
        Some(vec!["claude".into(), "codex".into()])
    );
}
#[test]
fn digits_highlight_and_only_visible_deliberate_enter_confirms() {
    let mut picker = ChoicePicker::new(
        vec![
            Choice::new("cancel", "Cancel"),
            Choice::new("apply", "Apply change"),
        ],
        Some("cancel"),
        SelectionMode::Single,
        Filter::Off,
    )
    .unwrap();
    let key = |code| Event::Key(KeyEvent::new(code, KeyModifiers::NONE));
    assert_eq!(accepted(&mut picker, &key(KeyCode::Char('2')), true), None);
    assert_eq!(picker.selected(), Some("apply"));
    assert_eq!(accepted(&mut picker, &key(KeyCode::Enter), false), None);
    let mut repeat = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
    repeat.kind = KeyEventKind::Repeat;
    assert_eq!(accepted(&mut picker, &Event::Key(repeat), true), None);
    assert_eq!(
        accepted(&mut picker, &key(KeyCode::Enter), true),
        Some("apply".into())
    );
}
#[test]
fn narrow_monochrome_review_keeps_safe_choice_and_explicit_apply_label() {
    let mut terminal = Terminal::new(TestBackend::new(32, 10)).unwrap();
    let mut picker = ChoicePicker::new(
        vec![
            Choice::new("cancel", "Cancel"),
            Choice::new("apply", "Apply change"),
        ],
        Some("cancel"),
        SelectionMode::Single,
        Filter::Off,
    )
    .unwrap();
    terminal
        .draw(|frame| {
            picker.render(
                frame,
                frame.area(),
                PickerView {
                    title: "Review",
                    context: "Project policy",
                    presentation: Presentation {
                        colors: ColorMode::NoColor,
                        ..Presentation::default()
                    },
                    ..PickerView::default()
                },
                true,
            );
        })
        .unwrap();
    let text = terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(crate::ratatui::buffer::Cell::symbol)
        .collect::<String>();
    assert!(text.contains("Cancel"));
    assert!(text.contains("Apply change"));
    assert_eq!(picker.selected(), Some("cancel"));
}
