//! Exercises complete composer workflows through public events and host callbacks.

use canonic_tui::{
    composer::{
        BoxMode, CompletionItem, CompletionKind, Composer, EditorResult, InputMode, Intent,
    },
    event::{Event, KeyCode, KeyEvent, KeyModifiers},
};
use std::time::Duration;

fn key(code: KeyCode) -> Event {
    Event::Key(KeyEvent::new(code, KeyModifiers::NONE))
}
fn ctrl(c: char) -> Event {
    Event::Key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL))
}
fn sequence(c: &mut Composer, code: KeyCode) {
    c.update(&ctrl('x'));
    c.update(&key(code));
}
fn queued(c: &mut Composer, text: &str) {
    c.set_text(text);
    assert!(c.update(&key(KeyCode::Enter)).is_none());
}

#[test]
fn rapid_unbracketed_paste_preserves_prefixes_and_newlines_without_execution() {
    let mut decoder = canonic_tui::composer::PasteBurst::default();
    let mut c = Composer::default();
    for (i, ch) in "/stash\n!pwd".chars().enumerate() {
        let event = if ch == '\n' {
            key(KeyCode::Enter)
        } else {
            key(KeyCode::Char(ch))
        };
        for event in decoder.push(event, Duration::from_millis(i as u64)) {
            assert!(c.update(&event).is_none());
        }
    }
    for event in decoder.flush_idle(Duration::from_millis(40)) {
        assert!(matches!(event, Event::Paste(_)));
        assert!(c.update(&event).is_none());
    }
    assert_eq!(c.editor().text(), "/stash\n!pwd");
    assert!(!c.overlay_open());
    assert_eq!(c.input_mode(), InputMode::Message);
    assert!(c.take_intents().is_empty());
    assert_eq!(
        decoder.push(key(KeyCode::Char('a')), Duration::from_millis(100)),
        Vec::<Event>::new()
    );
    assert_eq!(
        decoder.flush_idle(Duration::from_millis(120)),
        vec![key(KeyCode::Char('a'))]
    );
}

#[test]
fn completion_rejects_stale_results_and_loading_tab_without_losing_query() {
    let item = CompletionItem {
        id: "src".into(),
        value: "src/main.rs".into(),
        detail: String::new(),
        kind: CompletionKind::File,
        disabled: None,
        touched: true,
    };
    let mut c = Composer::default();
    assert!(c.set_completions(vec![item.clone()]));
    c.update(&key(KeyCode::Char('@')));
    let original = c.completion_request().unwrap();
    c.update(&key(KeyCode::Char('s')));
    let current = c.completion_request().unwrap();
    assert_ne!(original.ticket, current.ticket);
    assert!(!c.complete(&original, Ok(vec![])));
    assert!(c.set_completion_status(
        &current,
        canonic_tui::picker::Status::Loading("Searching".into())
    ));
    c.update(&key(KeyCode::Tab));
    assert_eq!(c.editor().text(), "@s");
    assert!(c.overlay_open());
    assert!(c.complete(&current, Ok(vec![item])));
    c.update(&key(KeyCode::Tab));
    assert_eq!(c.editor().expanded_text(), "@src/main.rs ");
    assert!(!c.overlay_open());
    assert!(!c.complete(&current, Err("late error")));
    assert!(c.take_intents().is_empty());
    c.set_text("old ");
    c.update(&key(KeyCode::Char('@')));
    c.update(&key(KeyCode::Char('s')));
    let request = c.completion_request().unwrap();
    c.update(&ctrl('z'));
    assert!(!c.overlay_open());
    assert_eq!(c.editor().text(), "old ");
    assert!(!c.complete(&request, Ok(vec![])));
}

#[test]
fn no_fit_completion_query_and_queued_hidden_cleanup_preserve_versions() {
    let mut c = Composer::new(2);
    c.update(&key(KeyCode::Char('@')));
    c.update(&key(KeyCode::Char('x')));
    let before = c.completion_request().unwrap();
    c.update(&key(KeyCode::Char('y')));
    assert_eq!(c.completion_request().unwrap(), before);
    assert_eq!(c.editor().text(), "@x");
    let mut c = Composer::default();
    c.set_working(true);
    queued(&mut c, "original");
    c.edit_queued(1);
    c.update(&Event::Paste("\u{200b}hidden".into()));
    c.update(&key(KeyCode::Enter));
    assert!(matches!(c.mode(), BoxMode::Queued { .. }));
    assert!(!c.editor().text().contains('\u{200b}'));
    c.update(&key(KeyCode::Enter));
    assert_eq!(c.queue()[0].text, "originalhidden");
}

#[test]
fn resize_does_not_disarm_empty_quit_confirmation() {
    let mut c = Composer::default();
    c.update(&ctrl('c'));
    c.update(&Event::Resize(40, 12));
    c.update(&ctrl('c'));
    let intents = c.take_intents();
    assert!(matches!(
        intents.as_slice(),
        [Intent::Save { .. }, Intent::Quit]
    ));
}

#[test]
fn external_editor_expands_text_chips_and_preserves_image_references() {
    let mut c = Composer::default();
    c.update(&Event::Paste("one\ntwo\nthree\nfour".into()));
    c.insert_image_with_size("image-reference", Some((640, 480)));
    let original = c.editor().document();
    let marker = *original.atoms.keys().next().unwrap();
    c.update(&Event::Paste(marker.to_string()));
    assert_eq!(c.editor().document().atoms.len(), 2);
    assert!(c.editor().text().ends_with('\u{fffd}'));
    let request = c.begin_editor("test").unwrap();
    assert!(request.text.starts_with("one\ntwo\nthree\nfour"));
    assert!(c.finish_editor(
        &request.ticket,
        EditorResult::Text(format!("{} edited", request.text))
    ));
    let document = c.editor().document();
    assert_eq!(document.atoms.len(), 1);
    assert_eq!(
        document.atoms.values().next().unwrap().reference.as_deref(),
        Some("image-reference")
    );
    assert!(c.editor().expanded_text().ends_with(" edited"));
}

#[test]
fn backspace_on_highlighted_chip_removes_it_and_undo_restores_payload() {
    let mut c = Composer::default();
    c.set_text("prefix ");
    c.update(&Event::Paste("one\ntwo\nthree\nfour".into()));
    let before = c.editor().document();
    c.update(&key(KeyCode::Left));
    c.update(&key(KeyCode::Backspace));
    assert_eq!(c.editor().text(), "prefix ");
    assert!(c.editor().document().atoms.is_empty());
    c.update(&ctrl('z'));
    assert_eq!(c.editor().document().atoms, before.atoms);
    assert_eq!(c.editor().expanded_text(), "prefix one\ntwo\nthree\nfour");
}

#[test]
fn queue_is_fifo_one_per_turn_and_held_edits_survive_renumbering() {
    let mut c = Composer::default();
    c.set_working(true);
    queued(&mut c, "first");
    queued(&mut c, "second");
    queued(&mut c, "third");
    c.set_text("unsent");
    let checkpoint = c.checkpoint();
    assert!(c.edit_queued(2));
    c.update(&Event::Paste(" edited".into()));
    let first = c.turn_ended(true).unwrap();
    assert_eq!(first.text, "first");
    assert_eq!(
        c.mode(),
        &BoxMode::Queued {
            position: 1,
            total: 2
        }
    );
    assert!(c.acknowledge(&first.ticket, Ok(())));
    let third = c.turn_ended(true).unwrap();
    assert_eq!(third.text, "third");
    assert!(c.acknowledge(&third.ticket, Ok(())));
    assert!(c.turn_ended(true).is_none());
    assert!(c.update(&key(KeyCode::Enter)).is_none());
    assert_eq!(c.checkpoint(), checkpoint);
    assert_eq!(c.queue()[0].text, "second edited");
    let second = c.turn_ended(true).unwrap();
    assert_eq!(second.text, "second edited");
    assert!(c.queue().is_empty());
}

#[test]
fn stopping_and_errors_pause_until_explicit_send_and_restart_never_sends() {
    let mut c = Composer::default();
    c.set_working(true);
    queued(&mut c, "first");
    c.set_text("preserved");
    c.update(&key(KeyCode::Esc));
    assert!(c.queue_paused());
    assert_eq!(c.editor().text(), "preserved");
    assert!(c.take_intents().iter().any(
        |i| matches!(i, Intent::Work(r) if r.action == canonic_tui::composer::WorkAction::Stop)
    ));
    assert!(c.turn_ended(true).is_none());
    let saved = c.saved();
    let mut restored = Composer::default();
    assert!(restored.restore_saved(&saved));
    assert!(restored.queue_paused());
    assert!(restored.take_intents().is_empty());
    assert!(restored.turn_ended(true).is_none());
    restored.update(&ctrl('x'));
    let submission = restored.update(&key(KeyCode::Enter)).unwrap();
    assert_eq!(submission.text, "first");
    assert!(restored.acknowledge(&submission.ticket, Ok(())));
    restored.set_working(true);
    queued(&mut restored, "after error");
    assert!(restored.turn_ended(false).is_none());
    assert!(restored.queue_paused());
}

#[test]
fn work_controls_wait_for_identified_acknowledgement_and_completion_wins() {
    use canonic_tui::composer::WorkAction;
    let mut c = Composer::default();
    c.set_working(true);
    queued(&mut c, "waiting");
    c.set_text("draft preserved");
    let checkpoint = c.checkpoint();
    c.update(&ctrl('c'));
    let request = c.work_request().unwrap().clone();
    assert_eq!(request.action, WorkAction::Stop);
    assert!(c.is_busy());
    assert!(c.status().contains("cancelling"));
    assert!(!c.status().contains("stopped"));
    c.update(&key(KeyCode::Esc));
    c.update(&ctrl('c'));
    assert_eq!(c.take_intents().len(), 1);
    assert_eq!(c.checkpoint(), checkpoint);
    assert!(
        c.acknowledge_work(&request.ticket, Err("host unavailable"))
            .0
    );
    assert!(c.is_busy());
    assert_eq!(c.checkpoint(), checkpoint);
    c.update(&key(KeyCode::Esc));
    let retry = c.work_request().unwrap().clone();
    assert_ne!(retry.ticket, request.ticket);
    assert!(!c.acknowledge_work(&request.ticket, Ok(())).0);
    assert!(c.turn_ended(true).is_none());
    assert!(!c.acknowledge_work(&retry.ticket, Ok(())).0);
    assert_eq!(c.checkpoint(), checkpoint);
    assert!(c.queue_paused());
}

#[test]
fn interrupt_and_background_only_release_foreground_after_host_confirmation() {
    use canonic_tui::composer::WorkAction;
    let mut c = Composer::default();
    c.set_working(true);
    queued(&mut c, "one");
    queued(&mut c, "two");
    c.set_text("draft");
    assert!(c.send_next_now().is_none());
    let request = c.work_request().unwrap().clone();
    assert_eq!(request.action, WorkAction::InterruptAfterStep);
    assert_eq!(c.queue().len(), 2);
    assert!(c.is_busy());
    let (accepted, sent) = c.acknowledge_work(&request.ticket, Ok(()));
    assert!(accepted);
    assert_eq!(sent.unwrap().text, "one");
    assert_eq!(c.queue().len(), 1);
    assert_eq!(c.editor().text(), "draft");
    assert!(!c.acknowledge_work(&request.ticket, Ok(())).0);

    let mut shell = Composer::default();
    shell.update(&key(KeyCode::Char('!')));
    shell.set_working(true);
    sequence(&mut shell, KeyCode::Char('b'));
    let request = shell.work_request().unwrap().clone();
    assert_eq!(request.action, WorkAction::Background);
    assert!(shell.is_busy());
    assert_eq!(shell.background_count(), 0);
    assert!(shell.acknowledge_work(&request.ticket, Ok(())).0);
    assert!(!shell.is_busy());
    assert_eq!(shell.background_count(), 1);
    assert!(shell.finish_background(&request.ticket));
    assert!(!shell.finish_background(&request.ticket));
}

#[test]
fn resume_waits_for_acceptance_and_requests_one_interrupt_before_sending() {
    use canonic_tui::composer::WorkAction;
    let mut c = Composer::default();
    c.set_text("pending");
    let first = c.submit().unwrap();
    queued(&mut c, "one");
    queued(&mut c, "two");
    assert!(c.send_next_now().is_none());
    assert!(c.status().contains("resuming"));
    assert!(c.acknowledge(&first.ticket, Ok(())));
    let work = c.work_request().unwrap().clone();
    assert_eq!(work.action, WorkAction::InterruptAfterStep);
    assert!(c.is_busy());
    assert!(c.send_next_now().is_none());
    let (accepted, submission) = c.acknowledge_work(&work.ticket, Ok(()));
    assert!(accepted);
    assert_eq!(submission.unwrap().text, "one");
    assert_eq!(c.queue().len(), 1);
}

#[test]
fn blank_queue_save_stashes_original_payload_and_editor_return_is_undoable() {
    let mut c = Composer::default();
    c.set_working(true);
    c.insert_image("host-image");
    assert!(c.update(&key(KeyCode::Enter)).is_none());
    c.set_text("independent draft");
    let checkpoint = c.checkpoint();
    assert!(c.edit_queued(1));
    c.update(&ctrl('g'));
    c.update(&key(KeyCode::Backspace));
    c.update(&key(KeyCode::Enter));
    assert!(c.queue().is_empty());
    assert_eq!(c.checkpoint(), checkpoint);
    assert!(c.restore_stash(1));
    assert_eq!(
        c.editor()
            .document()
            .atoms
            .values()
            .next()
            .unwrap()
            .reference
            .as_deref(),
        Some("host-image")
    );

    let mut c = Composer::default();
    c.set_text("original");
    c.update(&key(KeyCode::Left));
    let checkpoint = c.checkpoint();
    let request = c.begin_editor("nvim").unwrap();
    assert!(c.finish_editor(&request.ticket, EditorResult::Text("returned".into())));
    c.update(&ctrl('z'));
    assert_eq!(c.checkpoint(), checkpoint);
    let request = c.begin_editor("nvim").unwrap();
    c.update(&key(KeyCode::Esc));
    assert!(matches!(
        c.take_intents().last(),
        Some(Intent::EditorCancelled(_))
    ));
    assert!(!c.finish_editor(&request.ticket, EditorResult::Text("late".into())));
    assert_eq!(c.checkpoint(), checkpoint);
    assert!(c.status().contains("ignored"));
}

#[test]
fn file_completion_is_atomic_and_retains_host_identity_through_recovery() {
    use canonic_tui::{composer::SavedComposer, editor::AtomKind};
    let mut c = Composer::default();
    c.set_completions(vec![CompletionItem {
        id: "host-file-identity".into(),
        value: "src/digest.rs".into(),
        detail: String::new(),
        kind: CompletionKind::File,
        disabled: None,
        touched: false,
    }]);
    c.update(&key(KeyCode::Char('@')));
    c.update(&key(KeyCode::Tab));
    let document = c.editor().document();
    let atom = document.atoms.values().next().unwrap();
    assert_eq!(atom.kind, AtomKind::File);
    assert_eq!(atom.reference.as_deref(), Some("host-file-identity"));
    assert_eq!(c.editor().expanded_text(), "@src/digest.rs ");
    c.update(&key(KeyCode::Left));
    c.update(&key(KeyCode::Backspace));
    assert_eq!(c.editor().expanded_text(), " ");
    c.update(&ctrl('z'));
    assert_eq!(c.editor().document().atoms, document.atoms);
    c.stash_draft();
    let saved = c.saved();
    let mut bytes = Vec::new();
    saved.write_to(&mut bytes).unwrap();
    let decoded = SavedComposer::read_from(&mut bytes.as_slice(), 65_536).unwrap();
    assert_eq!(decoded, saved);
    assert!(c.restore_saved(&decoded));
    assert!(c.restore_stash(1));
    let editor = c.begin_editor("nvim").unwrap();
    assert!(c.finish_editor(&editor.ticket, EditorResult::Text(editor.text.clone())));
    assert_eq!(c.editor().document().atoms.values().next().unwrap(), atom);
    let sent = c.submit().unwrap();
    assert_eq!(sent.text, "@src/digest.rs ");
    assert_eq!(sent.document.atoms.values().next().unwrap(), atom);
    assert!(c.acknowledge(&sent.ticket, Err("offline")));
    assert_eq!(c.editor().document().atoms.values().next().unwrap(), atom);
    assert_eq!(c.editor().expanded_text(), "@src/digest.rs ");
}

#[test]
fn queued_cancel_keeps_original_and_remove_protects_the_edited_copy() {
    let mut c = Composer::default();
    c.set_working(true);
    queued(&mut c, "original");
    c.set_text("draft");
    assert!(c.edit_queued(1));
    c.update(&Event::Paste(" changed".into()));
    c.update(&key(KeyCode::Esc));
    assert_eq!(c.queue()[0].text, "original");
    assert_eq!(c.editor().text(), "draft");
    assert!(c.edit_queued(1));
    c.update(&Event::Paste(" changed".into()));
    c.update(&ctrl('d'));
    assert!(c.queue().is_empty());
    assert_eq!(c.editor().text(), "draft");
    assert_eq!(c.stashed_drafts()[0].text, "original changed");
}

#[test]
fn stash_swap_drop_undo_and_capacity_keep_the_newest_fifty() {
    let mut c = Composer::default();
    c.set_text("first");
    c.update(&key(KeyCode::Left));
    let original = c.checkpoint();
    sequence(&mut c, KeyCode::Char('s'));
    c.set_text("second");
    sequence(&mut c, KeyCode::Char('u'));
    assert_eq!(c.checkpoint(), original);
    sequence(&mut c, KeyCode::Char('u'));
    assert_eq!(c.editor().text(), "second");
    c.open_stash_list();
    c.update(&key(KeyCode::Char('d')));
    assert_eq!(c.stashed_drafts(), []);
    c.update(&key(KeyCode::Char('u')));
    assert_eq!(c.stashed_drafts()[0], original);
    c.update(&key(KeyCode::Esc));
    for i in 0..51 {
        c.set_text(&format!("draft {i}"));
        c.stash_draft();
    }
    let stash = c.stashed_drafts();
    assert_eq!(stash.len(), 50);
    assert_eq!(stash[0].text, "draft 50");
    assert_eq!(stash[49].text, "draft 1");
    assert!(c.editor().notice().unwrap().contains("oldest"));
}

#[test]
fn paste_and_image_atoms_are_atomic_and_survive_history_stash_restart_and_rejection() {
    let mut c = Composer::default();
    let text = "one\ntwo\nthree\nfour\nfive";
    c.update(&Event::Paste(text.into()));
    assert_eq!(c.editor().document().atoms.len(), 1);
    assert_eq!(c.editor().expanded_text(), text);
    c.update(&key(KeyCode::Backspace));
    assert_eq!(c.editor().text(), "");
    c.update(&ctrl('z'));
    assert_eq!(c.editor().expanded_text(), text);
    assert!(c.insert_image("acme-image"));
    let document = c.editor().document();
    c.stash_draft();
    assert!(c.restore_stash(1));
    assert_eq!(c.editor().document(), document);
    let saved = c.saved();
    let mut restored = Composer::default();
    assert!(restored.restore_saved(&saved));
    assert_eq!(restored.editor().document(), document);
    let sent = restored.update(&key(KeyCode::Enter)).unwrap();
    assert_eq!(sent.document.atoms.len(), 2);
    assert!(sent.text.starts_with(text));
    assert!(restored.acknowledge(&sent.ticket, Err("offline")));
    assert_eq!(restored.editor().document(), document);
    let retry = restored.update(&key(KeyCode::Enter)).unwrap();
    assert!(restored.acknowledge(&retry.ticket, Ok(())));
    assert!(restored.browse_history(1));
    let mut recalled = document;
    recalled.cursor = 0;
    assert_eq!(restored.editor().document(), recalled);
}

#[test]
fn autosave_debounces_edits_retries_failures_and_rejects_stale_acknowledgements() {
    let mut c = Composer::default();
    c.set_text("draft");
    c.tick(Duration::from_millis(499));
    assert!(c.take_intents().is_empty());
    c.tick(Duration::from_millis(1));
    let Intent::Save { ticket, state } = c.take_intents().pop().unwrap() else {
        panic!("save expected");
    };
    assert_eq!(state.draft.text, "draft");
    c.set_text("newer");
    assert!(c.acknowledge_save(&ticket, Err("disk full")));
    assert_eq!(c.editor().text(), "newer");
    c.tick(Duration::from_millis(500));
    assert!(c.take_intents().is_empty());
    c.tick(Duration::from_millis(4500));
    let Intent::Save {
        ticket: newer,
        state,
    } = c.take_intents().pop().unwrap()
    else {
        panic!("retry expected");
    };
    assert_eq!(state.draft.text, "newer");
    assert!(!c.acknowledge_save(&ticket, Ok(())));
    assert!(c.acknowledge_save(&newer, Ok(())));
    assert!(!c.acknowledge_save(&newer, Ok(())));
    let mut invalid = state;
    invalid.draft.cursor = 999;
    assert!(!c.restore_saved(&invalid));
    assert_eq!(c.editor().text(), "newer");
}

#[test]
fn editor_conflicts_stash_both_choices_and_cancellation_invalidates_returns() {
    for choose_editor in [true, false] {
        let mut c = Composer::default();
        c.set_text("original");
        let request = c.begin_editor("nvim").unwrap();
        c.update(&Event::Paste("ignored".into()));
        assert_eq!(c.editor().text(), "original");
        c.set_text("newer");
        assert!(c.finish_editor(&request.ticket, EditorResult::Text("returned".into())));
        assert!(c.overlay_open());
        c.tick(Duration::from_millis(300));
        c.update(&key(KeyCode::Char(if choose_editor { '1' } else { '2' })));
        assert!(c.overlay_open());
        assert_eq!(c.editor().text(), "newer");
        c.update(&key(KeyCode::Enter));
        assert_eq!(
            c.editor().text(),
            if choose_editor { "returned" } else { "newer" }
        );
        assert_eq!(
            c.stashed_drafts()[0].text,
            if choose_editor { "newer" } else { "returned" }
        );
        assert!(!c.finish_editor(&request.ticket, EditorResult::Text("late".into())));
    }
    let mut c = Composer::default();
    c.set_text("kept");
    let request = c.begin_editor("editor").unwrap();
    c.update(&ctrl('c'));
    assert!(!c.finish_editor(&request.ticket, EditorResult::Text("late".into())));
    assert_eq!(c.editor().text(), "kept");
    let request = c.begin_editor("editor").unwrap();
    assert!(c.finish_editor(&request.ticket, EditorResult::Text(String::new())));
    assert_eq!(c.editor().text(), "kept");
}

#[test]
fn escape_ladder_quit_deadline_and_rewind_only_on_send_are_explicit() {
    let mut c = Composer::default();
    c.set_text("draft");
    c.update(&key(KeyCode::Esc));
    assert_eq!(c.editor().text(), "draft");
    c.tick(Duration::from_millis(1500));
    c.update(&key(KeyCode::Esc));
    assert_eq!(c.editor().text(), "draft");
    c.update(&key(KeyCode::Esc));
    assert_eq!(c.editor().text(), "");
    c.update(&key(KeyCode::Esc));
    c.update(&key(KeyCode::Esc));
    assert_eq!(c.editor().text(), "draft");
    assert!(!c.take_intents().iter().any(|i| matches!(i, Intent::Rewind)));
    assert!(c.update(&key(KeyCode::Enter)).is_some());
    assert!(c.take_intents().iter().any(|i| matches!(i, Intent::Rewind)));
    let mut quit = Composer::default();
    quit.update(&ctrl('c'));
    assert!(quit.take_intents().is_empty());
    quit.update(&ctrl('c'));
    let intents = quit.take_intents();
    assert!(matches!(intents[0], Intent::Save { .. }));
    assert!(matches!(intents[1], Intent::Quit));
}

#[test]
fn shell_has_its_own_history_and_pasted_prefixes_never_trigger_modes() {
    let mut c = Composer::default();
    c.update(&key(KeyCode::Char('!')));
    assert_eq!(c.input_mode(), InputMode::Shell);
    c.set_text("cargo test");
    let sent = c.update(&key(KeyCode::Enter)).unwrap();
    assert_eq!(sent.mode, InputMode::Shell);
    assert!(c.acknowledge(&sent.ticket, Ok(())));
    assert_eq!(c.history(), &[] as &[String]);
    assert!(c.browse_history(1));
    assert_eq!(c.editor().text(), "cargo test");
    c.update(&key(KeyCode::Esc));
    c.update(&key(KeyCode::Backspace));
    assert_eq!(c.input_mode(), InputMode::Message);
    for prefix in ["!", "/", "@"] {
        c.set_text("");
        c.update(&Event::Paste(prefix.into()));
        assert_eq!(c.input_mode(), InputMode::Message);
        assert!(!c.overlay_open());
    }
}

#[test]
fn completions_do_not_submit_email_is_plain_and_hidden_characters_require_second_enter() {
    let mut c = Composer::default();
    c.set_completions(vec![CompletionItem {
        id: "acme-file".into(),
        value: "src/digest.rs".into(),
        detail: "touched".into(),
        kind: CompletionKind::File,
        disabled: None,
        touched: false,
    }]);
    c.update(&key(KeyCode::Char('@')));
    assert!(c.overlay_open());
    assert!(c.update(&key(KeyCode::Enter)).is_none());
    assert_eq!(c.editor().expanded_text(), "@src/digest.rs ");
    assert!(!c.overlay_open());
    c.set_text("sam");
    c.update(&key(KeyCode::Char('@')));
    assert!(!c.overlay_open());
    c.set_text("run\u{200b} tests 👩‍💻");
    assert!(c.update(&key(KeyCode::Enter)).is_none());
    assert_eq!(c.editor().text(), "run tests 👩‍💻");
    assert_eq!(c.update(&key(KeyCode::Enter)).unwrap().text, "run tests 👩‍💻");
}

#[test]
fn trailing_backslash_newline_is_one_undo_transaction_and_no_fit_paste_preserves_draft() {
    let mut c = Composer::default();
    c.set_text("one\\");
    assert!(c.update(&key(KeyCode::Enter)).is_none());
    assert_eq!(c.editor().text(), "one\n");
    c.update(&ctrl('z'));
    assert_eq!(c.editor().text(), "one\\");
    let mut tiny = Composer::new(4);
    tiny.set_text("kept");
    tiny.update(&Event::Paste("a\nb\nc\nd".into()));
    assert_eq!(tiny.editor().text(), "kept");
    assert!(tiny.editor().document().atoms.is_empty());
}
