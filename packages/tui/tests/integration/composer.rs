//! Verifies draft revisions, async races, recovery and instance isolation via public APIs.

use canonic_tui::{
    chat::{Action, Chat},
    composer::{BoxMode, Composer, DraftCheckpoint, State},
    editor::EditorView,
    event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers},
    ratatui::{
        Terminal,
        backend::TestBackend,
        style::{Color, Modifier},
    },
};

fn key(code: KeyCode, modifiers: KeyModifiers) -> Event {
    Event::Key(KeyEvent::new(code, modifiers))
}
fn enter() -> Event {
    key(KeyCode::Enter, KeyModifiers::NONE)
}

#[test]
fn up_edits_before_recall_then_browses_without_wrapping_and_down_restores_draft() {
    let mut composer = Composer::default();
    composer.set_history(vec!["newest".into(), "older\nsecond line".into()]);
    composer.set_text("one\ntwo");
    composer.update(&key(KeyCode::Up, KeyModifiers::NONE));
    assert_eq!(composer.editor().cursor(), 3);
    assert_eq!(composer.mode(), &BoxMode::Draft);
    composer.update(&key(KeyCode::Up, KeyModifiers::NONE));
    assert_eq!(composer.editor().cursor(), 0);
    let checkpoint = composer.checkpoint();
    composer.update(&key(KeyCode::Up, KeyModifiers::NONE));
    assert_eq!(
        composer.mode(),
        &BoxMode::History {
            position: 1,
            total: 2
        }
    );
    assert_eq!(composer.editor().text(), "newest");
    composer.update(&key(KeyCode::Up, KeyModifiers::NONE));
    assert_eq!(
        composer.mode(),
        &BoxMode::History {
            position: 2,
            total: 2
        }
    );
    assert_eq!(composer.editor().text(), "older\nsecond line");
    composer.update(&key(KeyCode::Up, KeyModifiers::NONE));
    assert_eq!(
        composer.mode(),
        &BoxMode::History {
            position: 2,
            total: 2
        }
    );
    composer.update(&key(KeyCode::Down, KeyModifiers::NONE));
    composer.update(&key(KeyCode::Down, KeyModifiers::NONE));
    assert_eq!(composer.mode(), &BoxMode::Draft);
    assert_eq!(composer.checkpoint(), checkpoint);
}

#[test]
fn history_copy_resends_with_fresh_ticket_preserves_draft_and_moves_accepted_text_to_top() {
    let mut composer = Composer::default();
    assert!(composer.set_history(vec!["newest".into(), "older".into()]));
    composer.set_text("unsent");
    composer.update(&key(KeyCode::Left, KeyModifiers::NONE));
    let original = composer.checkpoint();
    assert!(composer.browse_history(2));
    let submission = composer.update(&enter()).unwrap();
    assert_eq!(submission.text, "older");
    assert_eq!(composer.stashed_drafts(), vec![original]);
    assert_eq!(composer.editor().text(), "");
    assert!(composer.update(&enter()).is_none());
    assert_eq!(
        composer.history(),
        &["newest".to_owned(), "older".to_owned()]
    );
    assert!(composer.acknowledge(&submission.ticket, Ok(())));
    assert_eq!(
        composer.history(),
        &["older".to_owned(), "newest".to_owned()]
    );
    assert!(composer.browse_history(1));
    composer.update(&Event::Paste(" edited".into()));
    assert_eq!(composer.mode(), &BoxMode::Draft);
    assert_eq!(composer.history()[0], "older");
}

#[test]
fn invalid_history_load_and_active_browse_replacement_are_atomic() {
    let mut composer = Composer::new(6);
    assert!(composer.set_history(vec!["界界".into()]));
    for entries in [
        vec!["界界界".into()],
        vec!["x".into(); 501],
        vec!["a\r\nb".into()],
    ] {
        assert!(!composer.set_history(entries));
        assert_eq!(composer.history(), &["界界".to_owned()]);
    }
    assert!(composer.browse_history(1));
    assert!(!composer.set_history(vec!["other".into()]));
    assert_eq!(composer.editor().text(), "界界");
    assert!(composer.restore_draft());
    assert!(composer.set_history(Vec::new()));
    composer.update(&key(KeyCode::Up, KeyModifiers::NONE));
    assert_eq!(composer.mode(), &BoxMode::Draft);
    assert!(
        composer
            .editor()
            .notice()
            .unwrap()
            .contains("No sent messages")
    );
}

#[test]
fn recall_escape_restores_cursor_selection_undo_and_checkpoint() {
    let mut composer = Composer::default();
    composer.set_text("draft");
    composer.update(&Event::Paste("🙂".into()));
    composer.update(&key(KeyCode::Left, KeyModifiers::SHIFT));
    let checkpoint = composer.checkpoint();
    let selection = composer.editor().selection();
    assert!(composer.recall(
        BoxMode::History {
            position: 3,
            total: 12
        },
        "old\npasted content"
    ));
    assert_eq!(composer.checkpoint(), checkpoint);
    assert!(composer.update(&enter()).is_none());
    assert!(
        composer
            .update(&key(KeyCode::Esc, KeyModifiers::NONE))
            .is_none()
    );
    assert_eq!(composer.mode(), &BoxMode::Draft);
    assert_eq!(composer.editor().selection(), selection);
    assert_eq!(composer.checkpoint(), checkpoint);
    composer.update(&key(KeyCode::Char('z'), KeyModifiers::CONTROL));
    assert_eq!(composer.editor().text(), "draft");
}

#[test]
fn editing_history_protects_original_draft_and_rejected_edits_do_not_adopt() {
    let mut composer = Composer::new(12);
    composer.set_text("unsent draft");
    let checkpoint = composer.checkpoint();
    let mode = BoxMode::History {
        position: 1,
        total: 1,
    };
    assert!(composer.recall(mode.clone(), "012345678901"));
    composer.update(&Event::Paste("x".into()));
    assert_eq!(composer.mode(), &mode);
    assert_eq!(composer.checkpoint(), checkpoint);
    assert_eq!(composer.stashed_drafts(), Vec::<DraftCheckpoint>::new());
    composer.update(&key(KeyCode::Backspace, KeyModifiers::NONE));
    assert_eq!(composer.mode(), &BoxMode::Draft);
    assert_eq!(composer.stashed_drafts(), vec![checkpoint]);
    assert_eq!(composer.editor().text(), "01234567890");
}

#[test]
fn checkpoint_validation_is_atomic_and_restores_grapheme_cursor() {
    let mut composer = Composer::new(20);
    composer.set_text("kept");
    let old = composer.checkpoint();
    for checkpoint in [
        DraftCheckpoint {
            text: "界🙂".into(),
            cursor: 1,
        },
        DraftCheckpoint {
            text: "e\u{301}".into(),
            cursor: 1,
        },
        DraftCheckpoint {
            text: "too long a restored draft".into(),
            cursor: 0,
        },
        DraftCheckpoint {
            text: "short".into(),
            cursor: 99,
        },
    ] {
        assert!(!composer.restore_checkpoint(&checkpoint));
        assert_eq!(composer.checkpoint(), old);
    }
    let checkpoint = DraftCheckpoint {
        text: "界🙂\nnext".into(),
        cursor: 3,
    };
    assert!(composer.restore_checkpoint(&checkpoint));
    assert_eq!(composer.checkpoint(), checkpoint);
}

#[test]
fn full_protected_stash_evicts_oldest_and_keeps_newest_draft_with_notice() {
    let mut composer = Composer::default();
    for index in 0..51 {
        composer.set_text(&format!("protected {index}"));
        assert!(composer.recall(
            BoxMode::History {
                position: 1,
                total: 1
            },
            "history copy"
        ));
        composer.update(&Event::Paste(" edited".into()));
    }
    assert_eq!(composer.editor().text(), "history copy edited");
    let protected = composer.stashed_drafts();
    assert_eq!(protected.len(), 50);
    assert_eq!(protected[0].text, "protected 50");
    assert_eq!(protected[49].text, "protected 1");
    assert!(composer.editor().notice().unwrap().contains("oldest"));
}

#[test]
fn queued_foundation_and_locked_modes_cannot_submit_or_lose_draft() {
    let mut composer = Composer::default();
    composer.set_text("unsent");
    let old = composer.checkpoint();
    assert!(composer.recall(
        BoxMode::Queued {
            position: 2,
            total: 2
        },
        "queued text"
    ));
    composer.update(&Event::Paste("must wait for save contract".into()));
    assert!(composer.submit().is_none());
    assert_eq!(composer.editor().text(), "queued text");
    assert!(composer.restore_draft());
    assert_eq!(composer.checkpoint(), old);
    assert!(composer.lock("nvim"));
    composer.update(&Event::Paste("ignored".into()));
    composer.update(&key(KeyCode::Backspace, KeyModifiers::NONE));
    assert!(composer.update(&enter()).is_none());
    assert!(composer.submit().is_none());
    assert_eq!(composer.checkpoint(), old);
    assert!(composer.unlock());
    assert_eq!(composer.checkpoint(), old);
}

#[test]
fn recall_rules_keep_dock_cursor_position_and_work_without_color() {
    use canonic_tui::theme::{ColorMode, Presentation, Symbols};
    for colors in [
        ColorMode::TrueColor,
        ColorMode::Ansi256,
        ColorMode::Ansi16,
        ColorMode::NoColor,
    ] {
        for symbols in [Symbols::Unicode, Symbols::Ascii] {
            let mut composer = Composer::default();
            composer.set_text("draft");
            let view = EditorView {
                footer: "Acme session",
                presentation: Presentation {
                    colors,
                    symbols,
                    ..Presentation::default()
                },
                ..EditorView::default()
            };
            let mut terminal = Terminal::new(TestBackend::new(60, 5)).unwrap();
            terminal
                .draw(|f| composer.render_with(f, f.area(), view, true))
                .unwrap();
            let draft = super::components::dump(terminal.backend().buffer());
            assert!(draft.lines().next().unwrap().starts_with("message"));
            assert!(composer.recall(
                BoxMode::History {
                    position: 3,
                    total: 12
                },
                "history"
            ));
            terminal
                .draw(|f| composer.render_with(f, f.area(), view, true))
                .unwrap();
            let recalled = super::components::dump(terminal.backend().buffer());
            let rule = recalled.lines().next().unwrap();
            assert!(rule.starts_with("history"));
            assert!(rule.contains("3 of 12"));
            assert!(rule.contains(symbols.recall_rule()));
            assert_ne!(symbols.recall_rule(), symbols.rule());
            assert_eq!(draft.lines().last(), recalled.lines().last());
            if colors == ColorMode::NoColor {
                assert_eq!(terminal.backend().buffer()[(10, 0)].fg, Color::Reset);
            }
            assert!(composer.restore_draft());
            terminal
                .draw(|f| composer.render_with(f, f.area(), view, true))
                .unwrap();
            assert_eq!(super::components::dump(terminal.backend().buffer()), draft);
        }
    }
}

#[test]
fn refused_message_returns_only_into_an_empty_box() {
    for fresh in ["", "new message"] {
        let mut draft = Composer::default();
        draft.set_text("sent");
        let submission = draft.submit().unwrap();
        if !fresh.is_empty() {
            draft.set_text(fresh);
        }
        assert!(draft.acknowledge(&submission.ticket, Err("offline")));
        assert_eq!(
            draft.editor().text(),
            if fresh.is_empty() { "sent" } else { fresh }
        );
        let State::Rejected { submission, .. } = draft.state() else {
            panic!("expected rejection");
        };
        assert_eq!(submission.text, "sent");
    }
}

#[test]
fn sent_turn_is_fixed_with_dim_delivery_and_acceptance_only_removes_the_status() {
    let mut chat = Chat::new();
    chat.push("acme", "stream start");
    chat.set_input("sent text");
    let Action::Submit(submission) = chat.update(&enter()) else {
        panic!("submission");
    };
    chat.push_submission(&submission, "20:10");
    chat.append(" more output");
    assert_eq!(chat.input(), "");
    chat.update(&Event::Paste("fresh text".into()));
    let mut t = Terminal::new(TestBackend::new(70, 18)).unwrap();
    t.draw(|f| chat.render(f, f.area(), canonic_tui::theme::Theme::Dark))
        .unwrap();
    let pending = super::components::dump(t.backend().buffer());
    assert!(pending.contains("you · 20:10 · sending"));
    assert!(pending.contains("stream start more output"));
    assert!(!pending.contains("sent text more output"));
    let (row, line) = pending
        .lines()
        .enumerate()
        .find(|(_, line)| line.contains("you · 20:10 · sending"))
        .unwrap();
    let column = line.chars().position(|c| c == 's').unwrap();
    let cell = &t.backend().buffer()[(u16::try_from(column).unwrap(), u16::try_from(row).unwrap())];
    assert!(cell.modifier.contains(Modifier::DIM));
    assert_ne!(cell.fg, Color::Yellow);
    assert!(chat.acknowledge_submission(&submission.ticket, Ok(())));
    t.draw(|f| chat.render(f, f.area(), canonic_tui::theme::Theme::Dark))
        .unwrap();
    let accepted = super::components::dump(t.backend().buffer());
    assert!(accepted.contains("you · 20:10"));
    assert!(!accepted.contains("sending"));
    assert!(accepted.contains("sent text"));
    assert_eq!(chat.input(), "fresh text");
    assert!(!chat.acknowledge_submission(&submission.ticket, Err("late response")));
}

#[test]
fn rejection_does_not_replace_an_edited_empty_draft_or_its_undo() {
    let mut draft = Composer::default();
    draft.set_text("sent");
    let sent = draft.submit().unwrap();
    draft.update(&Event::Paste("fresh".into()));
    draft.update(&key(KeyCode::Char('g'), KeyModifiers::CONTROL));
    draft.update(&key(KeyCode::Backspace, KeyModifiers::NONE));
    assert!(draft.acknowledge(&sent.ticket, Err("offline")));
    assert_eq!(draft.editor().text(), "");
    draft.update(&key(KeyCode::Char('z'), KeyModifiers::CONTROL));
    assert_eq!(draft.editor().text(), "fresh");
}

#[test]
fn submission_clears_immediately_and_delayed_acceptance_preserves_the_fresh_box() {
    let mut draft = Composer::default();
    draft.set_text("hello\n界🙂");
    let submission = draft.update(&enter()).unwrap();
    assert_ne!(submission.version, *draft.version());
    assert_eq!(submission.text, "hello\n界🙂");
    assert_eq!(draft.editor().text(), "");
    let fresh_version = draft.version().clone();
    for _ in 0..3 {
        assert!(draft.update(&enter()).is_none());
    }
    // Navigation and rendering do not change the text revision.
    draft.update(&key(KeyCode::Left, KeyModifiers::NONE));
    let mut terminal = Terminal::new(TestBackend::new(40, 6)).unwrap();
    terminal
        .draw(|f| draft.render_with(f, f.area(), EditorView::default(), true))
        .unwrap();
    assert_eq!(draft.version(), &fresh_version);
    assert!(draft.acknowledge(&submission.ticket, Ok(())));
    assert_eq!(draft.editor().text(), "");
    assert_eq!(draft.state(), &State::Accepted);
    assert!(!draft.acknowledge(&submission.ticket, Err("late failure")));
    draft.update(&key(KeyCode::Char('z'), KeyModifiers::CONTROL));
    assert_eq!(draft.editor().text(), "", "sent text is not edit undo");
}

#[test]
fn acceptance_preserves_newer_edits_even_after_undo_to_identical_text() {
    for undo in [false, true] {
        let mut draft = Composer::default();
        draft.set_text("old");
        let submission = draft.submit().unwrap();
        draft.update(&Event::Paste(" newer".into()));
        if undo {
            draft.update(&key(KeyCode::Char('z'), KeyModifiers::CONTROL));
        }
        let text = draft.editor().text().to_owned();
        let cursor = draft.editor().cursor();
        assert_ne!(&submission.version, draft.version());
        assert!(draft.acknowledge(&submission.ticket, Ok(())));
        assert_eq!(draft.editor().text(), text);
        assert_eq!(draft.editor().cursor(), cursor);
        assert_eq!(draft.state(), &State::Accepted);
    }
}

#[test]
fn typing_after_submission_undoes_separately_from_the_submitted_text() {
    let mut draft = Composer::default();
    for character in "old".chars() {
        draft.update(&key(KeyCode::Char(character), KeyModifiers::NONE));
    }
    let submitted = draft.submit().unwrap();
    draft.update(&key(KeyCode::Char('x'), KeyModifiers::NONE));
    draft.update(&key(KeyCode::Char('z'), KeyModifiers::CONTROL));
    assert_eq!(draft.editor().text(), "");
    assert!(draft.acknowledge(&submitted.ticket, Ok(())));
    assert_eq!(draft.editor().text(), "", "undo is still a newer revision");
}

#[test]
fn rejection_retains_old_snapshot_and_new_draft_and_retry_has_a_new_ticket() {
    let mut draft = Composer::default();
    draft.set_text("original");
    let old = draft.submit().unwrap();
    draft.set_text("new draft");
    assert!(draft.acknowledge(&old.ticket, Err("network\x1b failure")));
    let State::Rejected { submission, reason } = draft.state() else {
        panic!("rejection");
    };
    assert_eq!(submission, &old);
    assert!(!reason.contains('\x1b'));
    assert_eq!(draft.editor().text(), "new draft");
    let retry = draft.submit().unwrap();
    assert_ne!(retry.ticket, old.ticket);
    assert!(!draft.acknowledge(&old.ticket, Ok(())));
    assert!(draft.acknowledge(&retry.ticket, Err("retry failed")));
    draft.set_text("new draft");
    let same_revision = draft.submit().unwrap();
    assert_ne!(retry.version, same_revision.version);
    assert_ne!(retry.ticket, same_revision.ticket);
    assert!(!draft.acknowledge(&retry.ticket, Ok(())));
    assert_eq!(draft.state(), &State::Pending(same_revision));
}

#[test]
fn lifetime_reset_and_foreign_responses_never_resolve_another_pending_draft() {
    let mut first = Composer::default();
    let mut second = Composer::default();
    first.set_text("one");
    second.set_text("two");
    let old = first.submit().unwrap();
    let foreign = second.submit().unwrap();
    assert!(!first.acknowledge(&foreign.ticket, Ok(())));
    first.reset("replacement");
    let new = first.submit().unwrap();
    assert_ne!(old.version.identity, new.version.identity);
    assert!(!first.acknowledge(&old.ticket, Ok(())));
    assert_eq!(first.editor().text(), "");
    assert_eq!(second.state(), &State::Pending(foreign));
    assert!(first.acknowledge(&new.ticket, Ok(())));
    assert_eq!(second.editor().text(), "");
}

#[test]
fn empty_blocked_modified_repeated_and_pasted_input_cannot_submit() {
    let mut draft = Composer::default();
    draft.set_text(" \n");
    assert!(draft.update(&enter()).is_none());
    draft.set_text("hello");
    for kind in [KeyEventKind::Repeat, KeyEventKind::Release] {
        assert!(
            draft
                .update(&Event::Key(KeyEvent::new_with_kind(
                    KeyCode::Enter,
                    KeyModifiers::NONE,
                    kind
                )))
                .is_none()
        );
    }
    for modifiers in [KeyModifiers::ALT, KeyModifiers::SHIFT] {
        assert!(draft.update(&key(KeyCode::Enter, modifiers)).is_none());
    }
    assert!(
        draft
            .update(&key(KeyCode::Char('j'), KeyModifiers::CONTROL))
            .is_none()
    );
    assert_eq!(draft.editor().text(), "hello\n\n\n");
    draft.set_blocked(Some("disconnected"));
    assert!(draft.update(&enter()).is_none());
    assert!(
        draft
            .update(&Event::Paste("/command\r\n!shell".into()))
            .is_none()
    );
    assert_eq!(draft.editor().text(), "hello\n\n\n/command\n!shell");
    draft.set_blocked(None);
    assert!(draft.update(&enter()).is_some());
}

#[test]
fn bounded_edit_rejection_does_not_advance_revision_or_disturb_pending_snapshot() {
    let mut draft = Composer::new(6);
    assert!(draft.set_text("界界界"));
    let submitted = draft.submit().unwrap();
    draft.set_text("界界");
    let version = draft.version().clone();
    draft.update(&Event::Paste("🙂".into()));
    assert_eq!(draft.version(), &version);
    assert!(draft.editor().rejected());
    assert_eq!(draft.state(), &State::Pending(submitted.clone()));
    for (width, height) in [(0, 0), (1, 1), (12, 5)] {
        let mut t = Terminal::new(TestBackend::new(width, height)).unwrap();
        t.draw(|f| draft.render_with(f, f.area(), EditorView::default(), true))
            .unwrap();
    }
    assert!(draft.acknowledge(&submitted.ticket, Ok(())));
    assert_eq!(draft.editor().text(), "界界");
}
