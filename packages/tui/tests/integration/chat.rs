//! Verifies identified rich turns, local focus, semantic actions and bounded navigation.

#[test]
fn paging_is_serial_across_directions_and_newer_failure_retries_with_a_fresh_request() {
    let mut chat = Chat::new();
    chat.set_input("protected draft");
    for n in 0..100 {
        chat.push_record(Turn::new(
            &format!("m-{n}"),
            Role::Assistant,
            "acme",
            "body",
        ));
    }
    chat.set_earlier_count(10);
    draw(&mut chat, 80, 24);
    chat.update(&alt(KeyCode::Home));
    chat.update(&alt(KeyCode::Home));
    let Intent::Earlier { request, .. } = chat.take_intents().pop().unwrap() else {
        panic!("earlier")
    };
    chat.update(&alt(KeyCode::End));
    assert_eq!(chat.take_intents(), [] as [Intent; 0]);
    assert!(chat.complete_earlier(
        &request,
        Ok(vec![Turn::new("older", Role::User, "you", "old")]),
        9
    ));
    chat.update(&alt(KeyCode::End));
    let Intent::Latest { request } = chat.take_intents().pop().unwrap() else {
        panic!("latest")
    };
    let anchor = chat.viewport_anchor().cloned();
    chat.update(&alt(KeyCode::Home));
    chat.update(&alt(KeyCode::Home));
    chat.update(&alt(KeyCode::End));
    assert_eq!(chat.take_intents(), [] as [Intent; 0]);
    assert_eq!(chat.viewport_anchor(), anchor.as_ref());
    assert!(draw(&mut chat, 80, 24).0.contains("loading newer messages"));
    assert!(chat.acknowledge(&request, Err("offline")));
    chat.tick(Duration::from_secs(10));
    let failed = draw(&mut chat, 80, 24).0;
    assert!(
        failed.contains("alt+end retry · could not load newer · offline"),
        "{failed}"
    );
    chat.update(&alt(KeyCode::End));
    let Intent::Latest { request: retry } = chat.take_intents().pop().unwrap() else {
        panic!("retry")
    };
    assert_ne!(retry, request);
    assert!(!chat.complete_latest(&request, vec![], 0));
    assert!(chat.complete_latest(
        &retry,
        vec![Turn::new("newest", Role::Assistant, "acme", "new")],
        9
    ));
    assert!(chat.following());
    assert!(!draw(&mut chat, 80, 24).0.contains("could not load newer"));
    assert_eq!(chat.input(), "protected draft");
}

#[test]
fn copy_and_open_show_operation_specific_pending_and_failure_before_success() {
    let mut chat = Chat::new();
    chat.push_record(Turn::new(
        "reply",
        Role::Assistant,
        "acme",
        "[reference](https://example.com/reference)",
    ));
    chat.set_input("protected draft");
    draw(&mut chat, 80, 24);
    chat.open_selection();
    chat.update(&key(KeyCode::Char('y')));
    let Intent::Copy { request, text, .. } = chat.take_intents().pop().unwrap() else {
        panic!("copy")
    };
    assert_eq!(text, "[reference](https://example.com/reference)");
    let pending = draw(&mut chat, 80, 24).0;
    assert!(pending.contains("copying · waiting for host"));
    assert!(pending.contains("esc done · copy pending"));
    chat.update(&key(KeyCode::Char('y')));
    assert_eq!(chat.take_intents(), [] as [Intent; 0]);
    assert!(chat.acknowledge(&request, Err("clipboard refused")));
    assert!(
        draw(&mut chat, 80, 24)
            .0
            .contains("couldn't copy · clipboard refused")
    );
    chat.update(&key(KeyCode::Char('o')));
    let Intent::Open { request, target } = chat.take_intents().pop().unwrap() else {
        panic!("open")
    };
    assert_eq!(target, "https://example.com/reference");
    let pending = draw(&mut chat, 100, 24).0;
    assert!(
        pending.contains("opening https://example.com/reference · waiting for host"),
        "{pending}"
    );
    assert!(!pending.contains("clipboard refused"));
    assert!(chat.acknowledge(&request, Err("scheme denied")));
    assert!(
        draw(&mut chat, 80, 24)
            .0
            .contains("not opened · scheme denied")
    );
    assert_eq!(chat.input(), "protected draft");
}

#[test]
fn notes_are_one_muted_line_and_welcome_distinguishes_host_suggestions_from_keys() {
    let mut chat = Chat::new();
    chat.set_welcome(canonic_tui::chat::Welcome {
        title: "canonic".into(),
        context: "workspace".into(),
        recent: vec![],
        suggestions: vec!["check the queue".into()],
    });
    let welcome = draw(&mut chat, 80, 24).0;
    assert!(welcome.contains("Suggested · from host"));
    assert!(welcome.find("check the queue").unwrap() < welcome.find("Try: / commands").unwrap());
    chat.push_record(Turn::new(
        "note",
        Role::Note,
        "hidden header",
        "– cancelled\nsecond line",
    ));
    let note = draw(&mut chat, 80, 24).0;
    assert!(
        note.lines().any(|line| line.starts_with("– cancelled")),
        "{note}"
    );
    assert!(!note.contains("hidden header"));
    assert!(!note.contains("second line"));
}

#[test]
fn large_shell_preview_retains_real_tail_and_full_copy_without_touching_draft() {
    let mut chat = Chat::new();
    chat.composer_mut().update(&key(KeyCode::Char('!')));
    chat.composer_mut()
        .update(&Event::Paste("printf output".into()));
    let submission = chat.composer_mut().update(&key(KeyCode::Enter)).unwrap();
    chat.push_submission(&submission, "20:03");
    assert!(chat.acknowledge_submission(&submission.ticket, Ok(())));
    let source = (0..5000)
        .map(|n| format!("output-{n:04}"))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(chat.set_shell_output(&submission.ticket, &source));
    chat.set_input("protected draft");
    let folded = draw(&mut chat, 80, 24).0;
    for expected in [
        "output-0000",
        "output-0002",
        "4995 lines folded",
        "output-4998",
        "output-4999",
    ] {
        assert!(folded.contains(expected), "{folded}");
    }
    assert!(!folded.contains("output-0003"));
    chat.open_selection();
    chat.update(&key(KeyCode::Char('y')));
    let Intent::Copy { text, request, .. } = chat.take_intents().pop().unwrap() else {
        panic!("shell copy");
    };
    assert_eq!(text, format!("```sh\n{source}\n```"));
    assert!(chat.acknowledge(&request, Ok("")));
    chat.update(&key(KeyCode::Char('c')));
    let Intent::Copy { text, request, .. } = chat.take_intents().pop().unwrap() else {
        panic!("shell code copy");
    };
    assert_eq!(text, source);
    assert!(chat.acknowledge(&request, Ok("")));
    chat.update(&key(KeyCode::Enter));
    let expanded = draw(&mut chat, 80, 24).0;
    assert!(!expanded.contains("4995 lines folded"));
    chat.update(&key(KeyCode::Enter));
    assert!(draw(&mut chat, 80, 24).0.contains("output-4999"));
    assert_eq!(chat.composer().editor().text(), "protected draft");
}

#[test]
fn diff_no_newline_markers_do_not_increment_lines_and_malformed_patches_stay_readable() {
    for (patch, expected) in [
        (
            "@@ -10,2 +10,2 @@\n-old\n+new\n\\ No newline at end of file\n context\n",
            "11   context",
        ),
        ("@@ old1 new2 @@\n+body", "malformed hunk header"),
        (
            "Binary files a/image and b/image differ",
            "patch has no text hunks",
        ),
    ] {
        let mut chat = Chat::new();
        let mut turn = Turn::new("reply", Role::Assistant, "acme", "");
        turn.parts = vec![Part::new(
            "patch",
            Content::Diff {
                file: "a.rs".into(),
                patch: patch.into(),
                summary: "supplied patch".into(),
            },
        )];
        chat.push_record(turn);
        let text = draw(&mut chat, 80, 30).0;
        assert!(text.contains(expected), "{text}");
        chat.open_selection();
        chat.update(&key(KeyCode::Char('y')));
        let Intent::Copy { text, .. } = chat.take_intents().pop().unwrap() else {
            panic!("patch copy");
        };
        assert_eq!(text, patch);
    }
}

#[test]
fn returning_from_a_bounded_earlier_window_requires_the_latest_request() {
    let mut chat = Chat::new();
    chat.set_input("protected draft");
    for n in 0..100 {
        chat.push_record(Turn::new(
            &format!("m-{n}"),
            Role::Assistant,
            "acme",
            "body",
        ));
    }
    chat.set_earlier_count(1);
    draw(&mut chat, 80, 24);
    chat.update(&alt(KeyCode::Home));
    chat.update(&alt(KeyCode::Home));
    let Intent::Earlier { request, .. } = chat.take_intents().pop().unwrap() else {
        panic!("earlier");
    };
    assert!(chat.complete_earlier(
        &request,
        Ok(vec![Turn::new("older", Role::Assistant, "acme", "old")]),
        0
    ));
    assert_eq!(chat.history_counts(), (0, 1));
    chat.update(&alt(KeyCode::End));
    let Intent::Latest { request } = chat.take_intents().pop().unwrap() else {
        panic!("latest");
    };
    assert!(!Chat::new().complete_latest(&request, vec![], 0));
    assert!(!chat.acknowledge(&request, Ok("")));
    assert!(chat.complete_latest(
        &request,
        vec![Turn::new("newest", Role::Assistant, "acme", "latest")],
        100
    ));
    assert_eq!(chat.history_counts(), (100, 0));
    assert_eq!(chat.input(), "protected draft");
    assert!(chat.following());
    assert!(!chat.complete_latest(&request, vec![], 0));
}

#[test]
fn pathological_layout_cache_is_bounded_and_rebuilds_the_manual_anchor() {
    let mut chat = Chat::new();
    for n in 0..24 {
        chat.push_record(Turn::new(
            &format!("m-{n}"),
            Role::Assistant,
            "acme",
            &format!("message {n} with repeated source content\n\n").repeat(1024),
        ));
    }
    let latest = draw(&mut chat, 40, 24).0;
    assert!(latest.contains("message 23"), "{latest}");
    assert!(chat.metrics().cached_rows <= 32_768);
    assert!(chat.metrics().cached_bytes <= 8 * 1024 * 1024);
    chat.update(&alt(KeyCode::Home));
    let earlier = draw(&mut chat, 40, 24).0;
    assert!(earlier.contains("message 0"), "{earlier}");
    assert!(chat.metrics().cached_bytes <= 8 * 1024 * 1024);
}

#[test]
fn stale_resync_cannot_replace_progress_or_reopen_an_ended_reply() {
    let mut chat = rich();
    let snapshot = chat.turn("reply").unwrap().clone();
    let mut bad = mutation(&chat, "body", "bad");
    bad.expected = 99;
    let Updated::Resync(request) = chat.apply(bad) else {
        panic!("resync");
    };
    assert_eq!(
        chat.apply(mutation(&chat, "body", " latest")),
        Updated::Applied
    );
    assert!(!chat.resynchronize(&request, snapshot));
    assert!(chat.finish_turn("reply", End::Stopped("cancelled".into()), "12s"));
    assert!(!chat.end_turn("reply", End::Live));
    assert!(matches!(
        chat.apply(mutation(&chat, "body", "late")),
        Updated::Resync(_)
    ));
    assert!(matches!(chat.turn("reply").unwrap().end, End::Stopped(_)));
    assert!(draw(&mut chat, 80, 30).0.contains("12s"));
}

#[test]
fn table_search_and_markdown_table_inspection_use_semantic_cells() {
    for width in [100, 40] {
        let mut chat = Chat::new();
        chat.push_record(Turn::new(
            "table",
            Role::Assistant,
            "acme",
            "| Call | Header |\n| --- | --- |\n| daily | timezone |\n| weekly | timezone |",
        ));
        draw(&mut chat, width, 28);
        chat.open_search();
        chat.update(&Event::Paste("timezone".into()));
        let (text, buffer) = draw(&mut chat, width, 28);
        assert_eq!(chat.search_position(), (1, 2));
        assert!(text.contains("timezone"));
        assert!(
            buffer
                .content
                .iter()
                .any(|cell| cell.modifier.contains(Modifier::UNDERLINED) && cell.symbol() == "t"),
            "{text}"
        );
        chat.close_search(true);
        chat.open_selection();
        chat.update(&key(KeyCode::Char('o')));
        let Intent::Inspect {
            content: Content::Table { headers, rows },
            ..
        } = chat.take_intents().pop().unwrap()
        else {
            panic!("table inspection");
        };
        assert_eq!(headers, ["Call", "Header"]);
        assert_eq!(rows.len(), 2);
    }
}

#[test]
fn search_reveals_hidden_diff_hunks_and_tool_output_then_refolds() {
    for content in [
        Content::Diff {
            file: "a.rs".into(),
            summary: "two hunks".into(),
            patch: "@@ -1 +1 @@\n-old\n+new\n@@ -9 +9 @@\n-before\n+timezone\n".into(),
        },
        Content::Tool {
            verb: "run".into(),
            target: "tests".into(),
            summary: "done".into(),
            output: "timezone".into(),
            open: None,
        },
    ] {
        let mut chat = Chat::new();
        let mut turn = Turn::new("reply", Role::Assistant, "acme", "");
        turn.parts = vec![Part::new("content", content)];
        chat.push_record(turn);
        assert!(!draw(&mut chat, 80, 30).0.contains("timezone"));
        chat.open_search();
        chat.update(&Event::Paste("timezone".into()));
        let (text, buffer) = draw(&mut chat, 80, 30);
        assert!(text.contains("timezone"), "{text}");
        assert_eq!(chat.search_position(), (1, 1));
        assert!(buffer.content.iter().any(|cell| cell.symbol() == "t"
            && (cell.bg != canonic_tui::ratatui::style::Color::Reset
                || cell.modifier.contains(Modifier::REVERSED))));
        chat.close_search(false);
        assert!(!draw(&mut chat, 80, 30).0.contains("timezone"));
    }
}

#[test]
fn ascii_changes_only_decoration_and_unavailable_targets_never_open() {
    let mut chat = Chat::new();
    let mut turn = Turn::new("reply", Role::Assistant, "acme", "Source → café 界");
    turn.parts.push(Part::new(
        "bad",
        Content::Attachment {
            name: "document".into(),
            size: "12 KB".into(),
            target: Some("javascript:alert(1)".into()),
            unavailable: "not available".into(),
        },
    ));
    chat.push_record(turn);
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
    terminal
        .draw(|f| {
            chat.render_docked_with(
                f,
                f.area(),
                Presentation {
                    colors: canonic_tui::theme::ColorMode::NoColor,
                    symbols: canonic_tui::theme::Symbols::Ascii,
                    ..Presentation::default()
                },
            );
        })
        .unwrap();
    let text = super::components::dump(terminal.backend().buffer());
    assert!(text.contains("Source → café 界"));
    chat.open_selection();
    chat.update(&key(KeyCode::Char('o')));
    assert_eq!(chat.take_intents(), [] as [canonic_tui::chat::Intent; 0]);
}

#[test]
#[cfg(feature = "gallery")]
fn gallery_keeps_copy_and_earlier_loading_as_independent_host_requests() {
    use canonic_tui::gallery::{Component, Gallery};
    let mut gallery = Gallery::new();
    gallery
        .open_scenario(Component::Chat, "earlier-loading")
        .unwrap();
    gallery.update(&ctrl('x'));
    gallery.update(&key(KeyCode::Char('v')));
    gallery.update(&key(KeyCode::Char('y')));
    let mut terminal = Terminal::new(TestBackend::new(100, 32)).unwrap();
    terminal.draw(|f| gallery.render(f)).unwrap();
    let text = super::components::dump(terminal.backend().buffer());
    assert!(
        text.contains("loading earlier messages") && text.contains("copying"),
        "{text}"
    );
    gallery.update(&key(KeyCode::F(5)));
    terminal.draw(|f| gallery.render(f)).unwrap();
    let text = super::components::dump(terminal.backend().buffer());
    assert!(!text.contains("loading earlier messages"));
    assert!(text.contains("copying"), "{text}");
    gallery.update(&key(KeyCode::F(5)));
    terminal.draw(|f| gallery.render(f)).unwrap();
    assert!(super::components::dump(terminal.backend().buffer()).contains("copied Markdown"));
    assert_eq!(gallery.chat_input(), "Independent draft · preserved caret");
}

#[test]
#[cfg(feature = "gallery")]
fn gallery_alignment_frames_exercise_delayed_effects_and_paging_recovery() {
    use canonic_tui::gallery::{Component, Gallery};
    for (scenario, expected) in [
        ("copy-pending", "copying · waiting for host"),
        ("copy-failed", "clipboard unavailable"),
        ("copy-markdown-confirmed", "copied Markdown"),
        ("link-pending", "opening https://example.com/reference"),
        ("link-refused", "not opened"),
        ("link-confirmed", "opened https://example.com/reference"),
        ("table-inspection-pending", "requesting full content"),
        ("inspection-failed", "full source unavailable"),
        ("patch-inspection-pending", "requesting full content"),
        ("earlier-loading", "loading earlier messages"),
        ("earlier-failed", "could not load earlier"),
        ("newer-loading", "loading newer messages"),
        ("newer-failed", "could not load newer"),
        ("newer-restored", "Newest loaded window restored"),
        ("retry-unavailable", "transport disconnected"),
        ("quiet-note", "– cancelled · question dismissed"),
        ("reasoning-unavailable", "no reasoning text supplied"),
        ("attachment-unknown", "unknown kind · unknown size"),
        ("raw-control-unicode", "Unicode: 界"),
        ("source-limit", "not shown"),
        ("search-ansi16", "timezone"),
        ("search-no-color", "timezone"),
        ("search-narrow-ascii", "esc back"),
        ("host-suggestions", "Suggested · from host"),
    ] {
        let mut gallery = Gallery::new();
        gallery.open_scenario(Component::Chat, scenario).unwrap();
        let mut terminal = Terminal::new(TestBackend::new(100, 32)).unwrap();
        terminal.draw(|f| gallery.render(f)).unwrap();
        let text = super::components::dump(terminal.backend().buffer());
        assert!(text.contains(expected), "{scenario}: {text}");
        assert_eq!(gallery.chat_input(), "Independent draft · preserved caret");
        if scenario == "copy-pending" {
            gallery.update(&key(KeyCode::F(6)));
            terminal.draw(|f| gallery.render(f)).unwrap();
            assert!(super::components::dump(terminal.backend().buffer()).contains("couldn't copy"));
            gallery.update(&key(KeyCode::Char('y')));
            gallery.update(&key(KeyCode::F(5)));
            terminal.draw(|f| gallery.render(f)).unwrap();
            assert!(
                super::components::dump(terminal.backend().buffer()).contains("copied Markdown")
            );
        }
        if scenario == "table-inspection-pending" {
            gallery.update(&key(KeyCode::F(5)));
            terminal.draw(|f| gallery.render(f)).unwrap();
            assert!(
                super::components::dump(terminal.backend().buffer())
                    .contains("Full loaded content")
            );
            gallery.update(&key(KeyCode::Esc));
            assert_eq!(gallery.chat_input(), "Independent draft · preserved caret");
        }
    }
}

#[test]
#[cfg(feature = "gallery")]
fn gallery_chat_scenarios_have_stable_snapshots_and_local_escape_routing() {
    use canonic_tui::gallery::{Component, Gallery};
    for (scenario, width, height) in [
        ("answer", 100, 40),
        ("table-wide", 40, 24),
        ("search-folded", 100, 32),
        ("select-code", 64, 30),
        ("actions-off", 100, 30),
        ("pane", 64, 30),
    ] {
        let mut gallery = Gallery::new();
        gallery.open_scenario(Component::Chat, scenario).unwrap();
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal.draw(|f| gallery.render(f)).unwrap();
        insta::assert_snapshot!(
            format!("chat_{scenario}"),
            super::components::dump(terminal.backend().buffer())
        );
        if matches!(scenario, "search-folded" | "select-code") {
            gallery.update(&key(KeyCode::Esc));
            assert_eq!(gallery.current(), "chat");
            gallery.update(&key(KeyCode::Esc));
            assert_eq!(gallery.current(), "scenarios");
        }
    }
}

#[test]
#[cfg(feature = "gallery")]
fn chat_gallery_cancel_pauses_queue_and_picker_return_preserves_caret() {
    use canonic_tui::gallery::{Component, Gallery};
    let mut gallery = Gallery::new();
    gallery.open_scenario(Component::Chat, "streaming").unwrap();
    gallery.update(&key(KeyCode::F(1)));
    let mut keys_view = Terminal::new(TestBackend::new(100, 30)).unwrap();
    keys_view.draw(|f| gallery.render(f)).unwrap();
    assert!(super::components::dump(keys_view.backend().buffer()).contains("type filters"));
    gallery.update(&key(KeyCode::Esc));
    assert_eq!(gallery.current(), "chat");
    gallery.update(&Event::Paste("next message".into()));
    gallery.update(&key(KeyCode::Enter));
    assert_eq!(gallery.chat_input(), "");
    gallery.update(&key(KeyCode::Esc));
    assert_eq!(gallery.current(), "chat");
    gallery.update(&Event::Paste("abc".into()));
    gallery.update(&key(KeyCode::Left));
    gallery.update(&key(KeyCode::F(10)));
    gallery.update(&key(KeyCode::F(10)));
    gallery.update(&Event::Paste("!".into()));
    assert_eq!(gallery.chat_input(), "ab!c");
    let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
    terminal.draw(|f| gallery.render(f)).unwrap();
    let text = super::components::dump(terminal.backend().buffer());
    assert!(text.to_lowercase().contains("paused"), "{text}");
    assert!(text.contains("next message"), "{text}");
}

use canonic_tui::{
    chat::{Change, Chat, Content, End, Intent, Mode, Part, Role, Turn, Update, Updated},
    event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers},
    ratatui::{Terminal, backend::TestBackend, style::Modifier},
    theme::Presentation,
};
use proptest::prelude::*;
use std::time::{Duration, Instant};

fn key(code: KeyCode) -> Event {
    Event::Key(KeyEvent::new(code, KeyModifiers::NONE))
}
fn alt(code: KeyCode) -> Event {
    Event::Key(KeyEvent::new(code, KeyModifiers::ALT))
}
fn ctrl(c: char) -> Event {
    Event::Key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL))
}
fn draw(
    chat: &mut Chat,
    width: u16,
    height: u16,
) -> (String, canonic_tui::ratatui::buffer::Buffer) {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal
        .draw(|f| chat.render_docked_with(f, f.area(), Presentation::default()))
        .unwrap();
    (
        super::components::dump(terminal.backend().buffer()),
        terminal.backend().buffer().clone(),
    )
}
fn rich() -> Chat {
    let mut chat = Chat::new();
    chat.set_retry_available(true);
    let mut turn = Turn::new(
        "reply",
        Role::Assistant,
        "acme",
        "## Digest timezone\n\nProfile sync sends **X-Timezone**.\n\nSee [the client](src/profile/api.ts:168).",
    );
    turn.timestamp = "20:03".into();
    turn.parts.extend([
        Part::new(
            "code",
            Content::Code {
                text: "headers: {\n\t'X-Timezone': user.timezone,\n},".into(),
                language: "ts".into(),
                file: "src/profile/api.ts".into(),
                line: Some(168),
            },
        ),
        Part::new(
            "reason",
            Content::Reasoning {
                summary: "thought for 12s".into(),
                text: "The timezone header is per call.".into(),
            },
        ),
        Part::new(
            "tail",
            Content::Markdown("Add a test for each call.".into()),
        ),
    ]);
    chat.push_record(turn);
    chat
}
fn mutation(chat: &Chat, part: &str, text: &str) -> Update {
    let revision = chat
        .turn("reply")
        .expect("reply fixture")
        .parts
        .iter()
        .find(|p| p.id == part)
        .expect("identified fixture part")
        .revision;
    Update {
        conversation: chat.conversation(),
        message: "reply".into(),
        part: part.into(),
        expected: revision,
        revision: revision + 1,
        change: Change::Append(text.into()),
    }
}

#[test]
fn identified_interleaved_updates_are_atomic_and_only_relayout_their_part() {
    let mut chat = rich();
    draw(&mut chat, 80, 30);
    let before = chat.metrics();
    let update = mutation(&chat, "code", "\n// added independently");
    assert_eq!(chat.apply(update.clone()), Updated::Applied);
    assert_eq!(chat.metrics().parses, before.parses + 1);
    assert_eq!(chat.metrics().layouts, before.layouts + 1);
    let unchanged = chat.turn("reply").unwrap().clone();
    let Updated::Resync(request) = chat.apply(update.clone()) else {
        panic!("duplicate revision must request resync");
    };
    assert_eq!(chat.turn("reply"), Some(&unchanged));
    let mut foreign = mutation(&chat, "body", "wrong conversation");
    foreign.conversation = Chat::new().conversation();
    assert_eq!(chat.apply(foreign), Updated::Foreign);
    let mut wrong = unchanged.clone();
    wrong.id = "wrong".into();
    assert!(!chat.resynchronize(&request, wrong));
    assert!(chat.resynchronize(&request, unchanged.clone()));
    assert!(!chat.resynchronize(&request, unchanged));
    assert!(draw(&mut chat, 80, 30).0.contains("reloaded this reply"));
    assert!(!chat.push_record(Turn::new("reply", Role::Assistant, "acme", "duplicate")));
}

#[test]
fn search_matches_loaded_semantics_reveals_folds_and_preserves_the_draft_caret() {
    let mut chat = rich();
    chat.set_input("and bump version");
    chat.update(&key(KeyCode::Left));
    let cursor = chat.composer().editor().cursor();
    draw(&mut chat, 80, 30);
    chat.update(&ctrl('f'));
    chat.update(&Event::Paste("TIMEZONE".into()));
    assert_eq!(chat.mode(), Mode::Search);
    assert_eq!(chat.search_position(), (1, 5));
    let (text, buffer) = draw(&mut chat, 80, 30);
    assert!(text.contains("opened to show a match"));
    assert!(text.contains("header is per call"));
    assert!(
        buffer
            .content()
            .iter()
            .any(|cell| cell.modifier.contains(Modifier::REVERSED))
    );
    assert_eq!(chat.input(), "and bump version");
    assert_eq!(chat.composer().editor().cursor(), cursor);
    for _ in 0..5 {
        chat.update(&key(KeyCode::Down));
    }
    assert_eq!(chat.search_position(), (1, 5));
    assert!(draw(&mut chat, 80, 30).0.contains("wrapped"));
    chat.update(&key(KeyCode::Esc));
    assert_eq!(chat.mode(), Mode::Compose);
    assert!(!draw(&mut chat, 80, 30).0.contains("header is per call"));
    chat.update(&Event::Paste("!".into()));
    assert_eq!(chat.input(), "and bump versio!n");
}

#[test]
fn search_escape_restores_the_logical_view_after_streaming_and_enter_keeps_a_match() {
    let mut chat = Chat::new();
    chat.push_record(Turn::new(
        "reply",
        Role::Assistant,
        "acme",
        &"Line about timezone.\n\n".repeat(100),
    ));
    draw(&mut chat, 64, 22);
    chat.update(&alt(KeyCode::PageUp));
    let anchor = chat.viewport_anchor().cloned().unwrap();
    chat.open_search();
    chat.update(&Event::Paste("timezone".into()));
    chat.update(&key(KeyCode::Down));
    chat.apply(mutation(&chat, "body", "\n\nAdditional streaming output."));
    chat.update(&key(KeyCode::Esc));
    assert_eq!(chat.viewport_anchor(), Some(&anchor));
    assert!(!chat.following());
    chat.open_search();
    chat.update(&Event::Paste("Additional".into()));
    let found = chat.viewport_anchor().cloned();
    chat.update(&key(KeyCode::Enter));
    assert_eq!(chat.viewport_anchor().cloned(), found);
    chat.update(&alt(KeyCode::End));
    assert!(chat.following());
    assert_eq!(chat.unread_count(), 0);
}

#[test]
fn selection_copies_original_code_and_waits_for_a_real_host_acknowledgement() {
    let mut chat = rich();
    draw(&mut chat, 24, 30);
    chat.open_selection();
    chat.update(&key(KeyCode::Up));
    chat.update(&key(KeyCode::Up));
    chat.update(&key(KeyCode::Char('c')));
    let Intent::Copy {
        request,
        text,
        kind,
        lines,
    } = chat.take_intents().pop().unwrap()
    else {
        panic!("copy code intent");
    };
    assert_eq!(text, "headers: {\n\t'X-Timezone': user.timezone,\n},");
    assert_eq!(kind, "code");
    assert_eq!(lines, 3);
    assert!(!chat.complete_latest(&request, vec![], 0));
    assert!(!draw(&mut chat, 80, 30).0.contains("copied code"));
    assert!(!Chat::new().acknowledge(&request, Ok("")));
    assert!(chat.acknowledge(&request, Err("clipboard denied")));
    assert!(draw(&mut chat, 80, 30).0.contains("clipboard denied"));
    chat.update(&key(KeyCode::Char('c')));
    let Intent::Copy { request, .. } = chat.take_intents().pop().unwrap() else {
        panic!("retry copy");
    };
    assert!(chat.acknowledge(&request, Ok("")));
    assert!(draw(&mut chat, 80, 30).0.contains("copied code · 3 lines"));
    assert!(!chat.acknowledge(&request, Ok("duplicate")));
    chat.update(&key(KeyCode::Char('o')));
    let Intent::Inspect { content, .. } = chat.take_intents().pop().unwrap() else {
        panic!("code inspection");
    };
    assert!(
        matches!(content, Content::Code { file, line: Some(168), .. } if file=="src/profile/api.ts")
    );
}

#[test]
fn whole_message_and_extended_block_copy_keep_markdown_without_decorations() {
    let mut chat = rich();
    draw(&mut chat, 80, 30);
    chat.open_selection();
    chat.update(&key(KeyCode::Char('m')));
    chat.update(&key(KeyCode::Char('y')));
    let Intent::Copy { text, request, .. } = chat.take_intents().pop().unwrap() else {
        panic!("Markdown copy");
    };
    assert!(text.contains("## Digest timezone"));
    assert!(text.contains("**X-Timezone**"));
    assert!(text.contains("[the client](src/profile/api.ts:168)"));
    assert!(text.contains("```ts"));
    assert!(!text.contains("20:03") && !text.contains('▌') && !text.contains('↩'));
    chat.acknowledge(&request, Ok(""));
    chat.update(&key(KeyCode::Up));
    chat.update(&Event::Key(KeyEvent::new(KeyCode::Up, KeyModifiers::SHIFT)));
    chat.update(&key(KeyCode::Char('y')));
    let Intent::Copy { text, .. } = chat.take_intents().pop().unwrap() else {
        panic!("range copy");
    };
    assert!(text.contains("```ts") && text.contains("header is per call"));
}

#[test]
fn paging_generations_preserve_loaded_rows_anchor_and_draft_through_failure_and_prepend() {
    let mut chat = rich();
    chat.set_input("my untouched draft");
    chat.set_earlier_count(340);
    draw(&mut chat, 64, 20);
    chat.update(&alt(KeyCode::Home));
    assert_eq!(chat.take_intents(), [] as [canonic_tui::chat::Intent; 0]);
    chat.update(&alt(KeyCode::Home));
    let anchor = chat.viewport_anchor().cloned();
    let Intent::Earlier { request, before } = chat.take_intents().pop().unwrap() else {
        panic!("earlier request");
    };
    assert_eq!(before, "reply");
    chat.update(&alt(KeyCode::Home));
    assert_eq!(chat.take_intents(), [] as [canonic_tui::chat::Intent; 0]);
    assert!(chat.complete_earlier(&request, Err("offline"), 340));
    assert!(draw(&mut chat, 64, 20).0.contains("offline"));
    chat.update(&alt(KeyCode::Home));
    let Intent::Earlier { request: next, .. } = chat.take_intents().pop().unwrap() else {
        panic!("retry");
    };
    assert!(!chat.complete_earlier(&request, Ok(vec![]), 0));
    let mut earlier = Turn::new("earlier", Role::User, "you", "Older history");
    earlier.end = End::Quiet;
    assert!(chat.complete_earlier(&next, Ok(vec![earlier]), 339));
    assert_eq!(chat.message_count(), 2);
    assert_eq!(chat.viewport_anchor(), anchor.as_ref());
    assert_eq!(chat.input(), "my untouched draft");
    assert!(!chat.complete_earlier(&next, Ok(vec![]), 0));
}

#[test]
fn manual_reading_reflows_at_a_source_anchor_and_unread_does_not_move_the_dock() {
    let mut chat = rich();
    draw(&mut chat, 64, 20);
    chat.update(&alt(KeyCode::Up));
    let anchor = chat.viewport_anchor().cloned().unwrap();
    let before = draw(&mut chat, 64, 20).0;
    chat.apply(mutation(&chat, "tail", " More live output."));
    let after = draw(&mut chat, 64, 20).0;
    assert_eq!(chat.viewport_anchor(), Some(&anchor));
    assert_eq!(chat.unread_count(), 1);
    let rule = |text: &str| {
        text.lines()
            .position(|line| line.starts_with("message "))
            .unwrap()
    };
    assert_eq!(rule(&before), rule(&after));
    draw(&mut chat, 40, 20);
    let resized = chat.viewport_anchor().unwrap();
    assert_eq!(resized.message, anchor.message);
    assert_eq!(resized.part, anchor.part);
    assert!(resized.offset <= anchor.offset);
    chat.update(&alt(KeyCode::End));
    assert!(chat.following());
    assert_eq!(chat.unread_count(), 0);
}

#[test]
fn retry_is_capability_gated_and_a_new_reply_cannot_overwrite_the_failure() {
    let mut chat = rich();
    chat.end_turn("reply", End::Failed("connection lost".into()));
    let failed = chat.turn("reply").cloned();
    chat.set_retry_available(false);
    chat.update(&ctrl('x'));
    chat.update(&key(KeyCode::Char('r')));
    assert_eq!(chat.take_intents(), [] as [canonic_tui::chat::Intent; 0]);
    chat.set_retry_available(true);
    chat.update(&ctrl('x'));
    chat.update(&key(KeyCode::Char('r')));
    let Intent::Retry { request, message } = chat.take_intents().pop().unwrap() else {
        panic!("retry request");
    };
    assert_eq!(message, "reply");
    chat.acknowledge(&request, Ok(""));
    chat.push_record(Turn::new("retry", Role::Assistant, "acme", "Fresh reply"));
    assert_eq!(chat.turn("reply"), failed.as_ref());
    chat.reset_transcript();
    assert!(!chat.acknowledge(&request, Ok("late")));
    assert_eq!(chat.message_count(), 0);
}

#[test]
fn timestamps_keep_day_rules_and_unknown_html_escape_sequences_are_safe_text() {
    let mut chat = rich();
    let mut note = Turn::new("note", Role::Note, "host", "session started");
    note.day = "today".into();
    note.end = End::Quiet;
    chat.push_record(note);
    let mut turn = Turn::new(
        "raw",
        Role::Assistant,
        "acme",
        "<details>Raw</details>\n\nEscape \x1b]52;c;BAD\x07\n\n~~literal~~",
    );
    turn.end = End::Quiet;
    turn.parts.push(Part::new(
        "unknown",
        Content::Unknown {
            kind: "chart".into(),
            description: "3 series".into(),
        },
    ));
    chat.push_record(turn);
    chat.update(&ctrl('x'));
    chat.update(&key(KeyCode::Char('t')));
    let (text, _) = draw(&mut chat, 80, 50);
    assert!(text.contains("today"));
    assert!(!text.contains("20:03"));
    assert!(text.contains("session started"));
    assert!(text.contains("<details>Raw</details>"));
    assert!(text.contains("␛]52;c;BAD"));
    assert!(!text.contains('\x1b'));
    assert!(!text.contains('\x07'));
    assert!(text.contains("~~literal~~"));
    assert!(text.contains("[chart · 3 series]"));
}

#[test]
fn narrow_tables_keep_every_column_and_inspection_is_an_explicit_host_intent() {
    let mut chat = Chat::new();
    let mut turn = Turn::new("table", Role::Assistant, "acme", "");
    turn.parts = vec![Part::new(
        "table",
        Content::Table {
            headers: vec![
                "call".into(),
                "timezone".into(),
                "tests".into(),
                "owner".into(),
            ],
            rows: vec![vec![
                "daily digest".into(),
                "yes".into(),
                "4".into(),
                "sam".into(),
            ]],
        },
    )];
    turn.end = End::Quiet;
    chat.push_record(turn);
    let (text, _) = draw(&mut chat, 40, 20);
    assert!(text.contains("table · 4 columns"));
    assert!(text.contains("timezone yes"));
    assert!(text.contains("owner sam"));
    chat.open_selection();
    chat.update(&key(KeyCode::Char('o')));
    let Intent::Inspect {
        content: Content::Table { headers, rows },
        ..
    } = chat.take_intents().pop().unwrap()
    else {
        panic!("full table inspection");
    };
    assert_eq!(headers.len(), 4);
    assert_eq!(rows[0][3], "sam");
}

#[test]
fn source_limits_evictions_and_layout_omissions_are_visible_and_unicode_safe() {
    let mut chat = Chat::new();
    for n in 0..101 {
        chat.push_record(Turn::new(
            &format!("message-{n}"),
            Role::Assistant,
            "acme",
            "text",
        ));
    }
    assert_eq!(chat.message_count(), 100);
    assert!(chat.turn("message-0").is_none());
    let mut turn = Turn::new("huge", Role::Assistant, "acme", &"界\n".repeat(40_000));
    turn.end = End::Quiet;
    chat.push_record(turn);
    let Content::Markdown(text) = &chat.turn("huge").unwrap().parts[0].content else {
        panic!("Markdown");
    };
    assert!(text.len() <= 65_536);
    let (rendered, _) = draw(&mut chat, 64, 22);
    assert!(rendered.contains("not shown"));
    assert!(chat.metrics().cached_rows <= 32_768);
    assert!(rendered.contains("earlier messages not loaded"));
}

#[test]
fn warm_animation_does_not_reparse_or_rewrap_and_one_revision_invalidates_one_part() {
    let mut chat = rich();
    draw(&mut chat, 80, 30);
    let before = chat.metrics();
    for _ in 0..20 {
        chat.tick(Duration::from_millis(80));
        draw(&mut chat, 80, 30);
    }
    assert_eq!(chat.metrics(), before);
    chat.apply(mutation(&chat, "tail", " Extra."));
    let updated = chat.metrics();
    assert_eq!(updated.parses, before.parses + 1);
    assert_eq!(updated.layouts, before.layouts + 1);
    draw(&mut chat, 40, 30);
    assert_eq!(chat.metrics().parses, updated.parses);
    assert!(chat.metrics().layouts > updated.layouts);
}

#[test]
fn repeated_copy_and_released_leaders_never_issue_host_operations() {
    let mut chat = rich();
    chat.open_selection();
    let mut repeated = KeyEvent::new(KeyCode::Char('y'), KeyModifiers::NONE);
    repeated.kind = KeyEventKind::Repeat;
    chat.update(&Event::Key(repeated));
    chat.update(&ctrl('c'));
    chat.update(&ctrl('o'));
    chat.update(&Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::SHIFT,
    )));
    assert_eq!(chat.take_intents(), [] as [canonic_tui::chat::Intent; 0]);
    chat.close_selection();
    chat.open_search();
    chat.update(&Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::ALT,
    )));
    assert_eq!(chat.mode(), Mode::Search);
    chat.close_search(false);
    let mut released = KeyEvent::new(KeyCode::Char('x'), KeyModifiers::CONTROL);
    released.kind = KeyEventKind::Release;
    chat.update(&Event::Key(released));
    chat.update(&key(KeyCode::Char('v')));
    assert_eq!(chat.mode(), Mode::Compose);
    assert_eq!(chat.input(), "v");
}

#[test]
fn bounded_long_history_measurement_reports_cold_and_warm_layout_work() {
    let start = Instant::now();
    let mut chat = Chat::new();
    for n in 0..100 {
        chat.push_record(Turn::new(
            &format!("m-{n}"),
            Role::Assistant,
            "acme",
            &"Timezone prose with Unicode 界 and a code reference.\n\n".repeat(100),
        ));
    }
    draw(&mut chat, 100, 40);
    let cold = start.elapsed();
    let metrics = chat.metrics();
    let start = Instant::now();
    for _ in 0..25 {
        draw(&mut chat, 100, 40);
    }
    let warm = start.elapsed() / 25;
    assert_eq!(chat.metrics(), metrics);
    eprintln!(
        "Chat: 100 turns / 550 KB: cold={cold:?}, warm={warm:?}, rows={}",
        metrics.cached_rows
    );
}

proptest! {
    #[test]
    fn arbitrary_unicode_reflow_and_search_never_mutate_the_draft(text in ".{0,300}", width in 0u16..100) {
        let mut chat = Chat::new(); chat.set_input("protected draft"); chat.push_record(Turn::new("reply", Role::Assistant, "acme", &text));
        draw(&mut chat, width, 16); chat.update(&alt(KeyCode::Up)); chat.open_search(); chat.update(&Event::Paste("a".into())); draw(&mut chat, width, 16);
        chat.close_search(false); prop_assert_eq!(chat.input(), "protected draft"); prop_assert!(chat.metrics().cached_rows <= 32_768);
    }
}
