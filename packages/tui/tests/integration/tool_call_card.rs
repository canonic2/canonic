//! Public lifecycle, rendering, concurrency and Chat effect/anchor contracts.

use canonic_tui::{
    chat::{Change, Chat, Content, Intent as ChatIntent, Part, Role, Turn, Update, Updated},
    event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers},
    ratatui::{
        Terminal,
        backend::TestBackend,
        layout::Rect,
        style::{Color, Modifier},
    },
    theme::{ColorMode, Presentation, Symbols},
    tool_call_card::{
        Call, Capabilities, Command, Effect, Output, State, ToolCallCard, ToolGroup, ToolGroupCard,
    },
};
use std::time::Duration;

fn key(code: KeyCode) -> Event {
    Event::Key(KeyEvent::new(code, KeyModifiers::NONE))
}
fn call(state: State, output: &str) -> Call {
    let mut c = Call::new("run-1", "run", "npm test -- digest");
    c.state = state;
    c.summary = "6 passed".into();
    c.output = Output::Text(output.into());
    c.elapsed = Duration::from_secs(4);
    c.capabilities = Capabilities {
        details: true,
        retry: true,
        copy: true,
    };
    c
}
fn render(
    card: &ToolCallCard,
    width: u16,
    theme: Presentation,
    selected: bool,
) -> (String, canonic_tui::ratatui::buffer::Buffer) {
    let mut terminal = Terminal::new(TestBackend::new(width, 36)).unwrap();
    terminal
        .draw(|f| card.render_with(f, f.area(), theme, selected))
        .unwrap();
    (
        super::components::dump(terminal.backend().buffer()),
        terminal.backend().buffer().clone(),
    )
}
fn group_render(group: &ToolGroupCard) -> String {
    let mut terminal = Terminal::new(TestBackend::new(100, 48)).unwrap();
    terminal
        .draw(|f| group.render_with(f, f.area(), Presentation::default()))
        .unwrap();
    super::components::dump(terminal.backend().buffer())
}
fn chat(content: Content) -> Chat {
    let mut chat = Chat::new();
    let mut turn = Turn::new("reply", Role::Assistant, "acme", "");
    turn.parts = vec![Part::new("tools", content)];
    chat.push_record(turn);
    chat.set_input("and bump the version");
    chat
}
fn chat_render(chat: &mut Chat, width: u16) -> String {
    let mut terminal = Terminal::new(TestBackend::new(width, 36)).unwrap();
    terminal
        .draw(|f| chat.render_docked_with(f, f.area(), Presentation::default()))
        .unwrap();
    super::components::dump(terminal.backend().buffer())
}
fn replace(chat: &mut Chat, content: Content) -> Updated {
    let revision = chat.turn("reply").expect("seeded reply").parts[0].revision;
    chat.apply(Update {
        conversation: chat.conversation(),
        message: "reply".into(),
        part: "tools".into(),
        expected: revision,
        revision: revision + 1,
        change: Change::Replace(content),
    })
}
#[test]
fn lifecycle_rejects_wrong_ids_stale_revisions_and_resurrection() {
    let mut c = call(State::Queued, "");
    let mut card = ToolCallCard::new(c.clone()).unwrap();
    assert!(!card.replace(c.clone()));
    c.id = "foreign".into();
    c.revision = 1;
    c.state = State::Running;
    assert!(!card.replace(c.clone()));
    c.id = "run-1".into();
    assert!(card.replace(c.clone()));
    c.revision += 1;
    c.state = State::Waiting("index lock".into());
    assert!(card.replace(c.clone()));
    c.revision += 1;
    c.state = State::Completed;
    assert!(card.replace(c.clone()));
    c.revision += 1;
    c.state = State::Running;
    assert!(!card.replace(c));
    assert_eq!(card.call().state, State::Completed);
}
#[test]
fn all_states_have_words_and_queued_reserves_the_second_row() {
    for (state, expected) in [
        (State::Queued, "queued"),
        (State::Running, "running · 4s"),
        (
            State::Waiting("your approval".into()),
            "waiting for your approval",
        ),
        (State::Completed, "→ ✓ 6 passed"),
        (State::Failed, "→ ✕ 6 passed"),
        (
            State::Cancelled("you pressed esc".into()),
            "cancelled · you pressed esc",
        ),
    ] {
        let card = ToolCallCard::new(call(state, "")).unwrap();
        assert!(
            render(&card, 100, Presentation::default(), false)
                .0
                .contains(expected)
        );
        assert_eq!(card.height(100, Symbols::Unicode), 2);
    }
}
#[test]
fn failure_preview_expand_collapse_and_copy_keep_all_source() {
    use std::fmt::Write;
    let mut output = String::new();
    for i in 0..42 {
        let _ = writeln!(output, "error {i}");
    }
    let mut card = ToolCallCard::new(call(State::Failed, &output)).unwrap();
    let initial = render(&card, 100, Presentation::default(), false).0;
    assert!(
        initial.contains("error 0")
            && initial.contains("error 2")
            && initial.contains("39 more lines")
    );
    assert!(!initial.contains("error 3 "));
    card.command(Command::Toggle);
    let expanded = render(&card, 100, Presentation::default(), false).0;
    assert!(
        expanded.contains("error 9")
            && expanded.contains("error 41")
            && expanded.contains("22 more lines")
    );
    let intent = card.command(Command::CopyOutput).unwrap();
    assert_eq!(
        intent.effect,
        Effect::Copy {
            text: output,
            output_only: true
        }
    );
    assert!(card.acknowledge(&intent.ticket, Ok("")));
    card.command(Command::Toggle);
    assert!(
        !render(&card, 100, Presentation::default(), false)
            .0
            .contains("error 0")
    );
}
#[test]
fn streaming_coalesces_tail_and_flushes_terminal_outcome() {
    let mut c = call(State::Running, "old 1\nold 2\nold 3\nold 4\n");
    let mut card = ToolCallCard::new(c.clone()).unwrap();
    let height = card.height(100, Symbols::Unicode);
    c.revision += 1;
    c.output = Output::Text("new 1\nnew 2\nnew 3\nnew 4\nnew 5\n".into());
    assert!(card.replace(c.clone()));
    let pending = render(&card, 100, Presentation::default(), false).0;
    assert!(pending.contains("old 4") && pending.contains("5 lines"));
    assert!(!card.tick(Duration::from_millis(99)));
    assert!(card.tick(Duration::from_millis(1)));
    assert!(
        render(&card, 100, Presentation::default(), false)
            .0
            .contains("new 5")
    );
    assert_eq!(card.height(100, Symbols::Unicode), height);
    c.revision += 1;
    c.state = State::Completed;
    c.output = Output::Text("final".into());
    assert!(card.replace(c));
    assert!(
        render(&card, 100, Presentation::default(), false)
            .0
            .contains("final")
    );
}
#[test]
fn capabilities_pending_tickets_and_refusal_are_independent() {
    let mut card = ToolCallCard::new(call(State::Failed, "diagnostic")).unwrap();
    let foreign = ToolCallCard::new(call(State::Failed, "diagnostic"))
        .unwrap()
        .command(Command::Retry)
        .unwrap();
    let intent = card.command(Command::Retry).unwrap();
    assert!(!card.acknowledge(&foreign.ticket, Ok("")));
    assert!(card.command(Command::Retry).is_none());
    assert!(!card.hint().contains("r retry"));
    assert!(card.acknowledge(&intent.ticket, Err("host refused")));
    assert!(card.notice().contains("host refused"));
    assert!(!card.acknowledge(&intent.ticket, Ok("")));
    assert!(card.hint().contains("r retry"));
    let mut c = card.call().clone();
    c.revision += 1;
    let stale = card.command(Command::Details).unwrap();
    assert!(card.replace(c));
    assert!(!card.acknowledge(&stale.ticket, Ok("")));
    let mut disabled = call(State::Failed, "");
    disabled.capabilities = Capabilities::default();
    let mut disabled = ToolCallCard::new(disabled).unwrap();
    for command in [
        Command::CopyCall,
        Command::CopyOutput,
        Command::Details,
        Command::Retry,
    ] {
        assert!(disabled.command(command).is_none());
    }
    assert_eq!(disabled.hint(), "esc done");
}
#[test]
fn running_details_are_enabled_but_retry_is_not() {
    let mut card = ToolCallCard::new(call(State::Running, "tail")).unwrap();
    assert!(card.command(Command::Retry).is_none());
    assert_eq!(
        card.command(Command::Details).unwrap().effect,
        Effect::Details
    );
}
#[test]
fn code_diff_numbers_are_display_only_and_narrow_signs_wrap() {
    let mut c = call(State::Completed, "");
    c.verb = "read".into();
    c.target = "src/digest/settings.ts".into();
    c.output = Output::Code {
        text: "export function digestHeaders(user) {\n  return user.timezone;\n}".into(),
        first_line: 14,
    };
    let mut card = ToolCallCard::new(c).unwrap();
    assert!(
        !render(&card, 100, Presentation::default(), false)
            .0
            .contains("export")
    );
    card.command(Command::Toggle);
    assert!(
        render(&card, 100, Presentation::default(), false)
            .0
            .contains("14 export")
    );
    assert!(
        !render(&card, 40, Presentation::default(), false)
            .0
            .contains("14 export")
    );
    let copied = card.command(Command::CopyOutput).unwrap();
    assert!(
        matches!(copied.effect, Effect::Copy { text, .. } if text.starts_with("export") && !text.contains("│"))
    );
    let patch = "@@ -18,1 +18,1 @@\n-json,\n+json, 'X-Timezone': user.timezone,\n";
    let mut c = call(State::Completed, "");
    c.verb = "edit".into();
    c.target = "src/digest/settings.ts".into();
    c.output = Output::Diff(patch.into());
    let mut card = ToolCallCard::new(c).unwrap();
    card.command(Command::Toggle);
    let narrow = render(&card, 25, Presentation::default(), false).0;
    assert!(narrow.contains("− json,") && narrow.contains("+ json,") && narrow.contains('↩'));
    assert!(narrow.contains("settings.ts"));
    let copy = card.command(Command::CopyOutput).unwrap();
    assert_eq!(
        copy.effect,
        Effect::Copy {
            text: patch.into(),
            output_only: true
        }
    );
}
#[test]
fn unknown_binary_empty_controls_and_byte_caps_are_explicit() {
    let mut c = call(
        State::Completed,
        "\u{1b}]8;;https://example.com\u{7}link\r\u{009b}2J",
    );
    c.verb = "tracker".into();
    c.source = "tracker".into();
    c.name = "get_issue".into();
    c.arguments = "id: APP-22".into();
    let mut card = ToolCallCard::new(c).unwrap();
    card.command(Command::Toggle);
    let text = render(&card, 100, Presentation::default(), false).0;
    assert!(
        text.contains("tool")
            && text.contains("get_issue")
            && text.contains('␛')
            && text.contains("[U+0007]")
    );
    assert!(!card.call().output.text().contains('\u{1b}'));
    let mut c = call(State::Completed, "");
    c.output = Output::Unavailable("binary, 2.1 MB".into());
    let card = ToolCallCard::new(c).unwrap();
    assert!(
        render(&card, 100, Presentation::default(), false)
            .0
            .contains("binary, 2.1 MB · not shown")
    );
    let card = ToolCallCard::new(call(State::Completed, &"界".repeat(400_000))).unwrap();
    assert!(card.call().omitted_bytes > 0);
    assert!(card.call().output.text().len() <= 1024 * 1024);
    let mut running = call(State::Running, &"output\n".repeat(180_000));
    running.revision = 1;
    let card = ToolCallCard::new(running).unwrap();
    assert_eq!(card.call().total_lines, 180_000);
    assert!(
        render(&card, 100, Presentation::default(), false)
            .0
            .contains("180000 lines")
    );
}
#[test]
fn fallback_styles_keep_failure_bold_cancel_plain_and_selected_head_reversed() {
    let theme = Presentation {
        colors: ColorMode::NoColor,
        symbols: Symbols::Ascii,
        ..Default::default()
    };
    let failed = ToolCallCard::new(call(State::Failed, "failure")).unwrap();
    let (text, buffer) = render(&failed, 40, theme, true);
    assert!(text.contains("-> x 6 passed"));
    assert!(
        buffer
            .content
            .iter()
            .all(|c| c.fg == Color::Reset && c.bg == Color::Reset)
    );
    assert!(buffer[(0, 0)].modifier.contains(Modifier::REVERSED));
    assert!(
        buffer
            .content
            .iter()
            .any(|c| c.symbol() == "x" && c.modifier.contains(Modifier::BOLD))
    );
    let cancelled = ToolCallCard::new(call(State::Cancelled("host".into()), "")).unwrap();
    let (_, buffer) = render(&cancelled, 40, theme, false);
    assert!(
        buffer
            .content
            .iter()
            .filter(|c| c.symbol() == "x")
            .all(|c| !c.modifier.contains(Modifier::BOLD))
    );
    let (_, buffer) = render(
        &failed,
        100,
        Presentation {
            colors: ColorMode::Ansi16,
            ..Default::default()
        },
        false,
    );
    assert!(
        buffer
            .content
            .iter()
            .all(|c| !matches!(c.fg, Color::Indexed(_) | Color::Rgb(_, _, _)))
    );
}
#[test]
fn concurrent_completion_preserves_order_failure_and_local_expansion() {
    let mut a = call(State::Running, "a output");
    a.id = "a".into();
    a.target = "first".into();
    let mut b = call(State::Running, "b output");
    b.id = "b".into();
    b.target = "second".into();
    let mut snapshot = ToolGroup {
        id: "group".into(),
        revision: 0,
        calls: vec![a, b],
    };
    let mut group = ToolGroupCard::new(snapshot.clone()).unwrap();
    group.card_mut("a").unwrap().command(Command::Toggle);
    snapshot.revision = 1;
    snapshot.calls[1].revision = 1;
    snapshot.calls[1].state = State::Failed;
    assert!(group.replace(snapshot.clone()));
    let text = group_render(&group);
    assert!(text.find("first").unwrap() < text.find("second").unwrap());
    assert_eq!(
        group.card("a").unwrap().expansion(),
        canonic_tui::tool_call_card::Expansion::Expanded
    );
    snapshot.revision = 2;
    snapshot.calls[0].revision = 1;
    snapshot.calls[0].state = State::Completed;
    assert!(group.replace(snapshot.clone()));
    let text = group_render(&group);
    assert!(text.contains("second") && !text.contains("first") && text.contains("+1 more"));
    group.toggle();
    assert!(group_render(&group).contains("first"));
    snapshot.revision = 3;
    snapshot.calls.reverse();
    assert!(group.replace(snapshot));
    assert_eq!(
        group.card("a").unwrap().expansion(),
        canonic_tui::tool_call_card::Expansion::Expanded
    );
    let reordered = group_render(&group);
    assert!(reordered.find("second").unwrap() < reordered.find("first").unwrap());
    // Direct member updates cannot be resurrected by a newer group envelope.
    let mut live = call(State::Running, "");
    live.id = "member".into();
    let mut envelope = ToolGroup {
        id: "direct".into(),
        revision: 0,
        calls: vec![live.clone()],
    };
    let mut direct = ToolGroupCard::new(envelope.clone()).unwrap();
    live.revision = 2;
    live.state = State::Completed;
    assert!(direct.card_mut("member").unwrap().replace(live));
    envelope.revision = 1;
    envelope.calls[0].revision = 3;
    assert!(!direct.replace(envelope));
}
#[test]
fn successful_groups_fold_and_large_groups_have_a_counted_limit() {
    let mut c = call(State::Completed, "");
    c.id = "a".into();
    let mut other = c.clone();
    other.id = "b".into();
    let mut group = ToolGroupCard::new(ToolGroup {
        id: "group".into(),
        revision: 0,
        calls: vec![c.clone(), other],
    })
    .unwrap();
    assert!(!group_render(&group).contains("npm test"));
    group.toggle();
    assert!(group_render(&group).contains("npm test"));
    let calls = (0..10)
        .map(|i| {
            let mut c = c.clone();
            c.id = i.to_string();
            c.target = format!("task-{i}");
            c.state = State::Running;
            c
        })
        .collect();
    let mut group = ToolGroupCard::new(ToolGroup {
        id: "large".into(),
        revision: 0,
        calls,
    })
    .unwrap();
    let text = group_render(&group);
    assert!(text.contains("task-4") && !text.contains("task-5") && text.contains("+5 more"));
    group.toggle();
    assert!(group_render(&group).contains("task-9"));
    assert!(
        ToolGroupCard::new(ToolGroup {
            id: "duplicate".into(),
            revision: 0,
            calls: vec![c.clone(), c]
        })
        .is_err()
    );
}
#[test]
fn chat_select_effects_preserve_draft_caret_anchor_and_stale_tickets() {
    let mut chat = chat(Content::ToolCard(Box::new(call(
        State::Completed,
        "line 1\nline 2",
    ))));
    chat.update(&key(KeyCode::Left));
    chat_render(&mut chat, 100);
    chat.open_selection();
    let before = chat.viewport_anchor().cloned();
    chat.update(&key(KeyCode::Enter));
    assert_eq!(chat.viewport_anchor().cloned(), before);
    chat.update(&key(KeyCode::Char('c')));
    chat.update(&Event::Key(KeyEvent {
        kind: KeyEventKind::Repeat,
        ..KeyEvent::new(KeyCode::Char('c'), KeyModifiers::NONE)
    }));
    let [
        ChatIntent::Tool {
            message,
            part,
            intent,
        },
    ]: [ChatIntent; 1] = chat.take_intents().try_into().unwrap()
    else {
        panic!("card effect");
    };
    assert_eq!(
        intent.effect,
        Effect::Copy {
            text: "line 1\nline 2".into(),
            output_only: true
        }
    );
    assert!(chat.acknowledge_tool(
        &message,
        &part,
        &intent.call,
        &intent.ticket,
        Err("clipboard unavailable")
    ));
    assert!(chat_render(&mut chat, 100).contains("clipboard unavailable"));
    chat.close_selection();
    chat.update(&key(KeyCode::Char('!')));
    assert_eq!(chat.input(), "and bump the versio!n");
    assert!(!chat.acknowledge_tool(&message, &part, &intent.call, &intent.ticket, Ok("")));
    chat.open_selection();
    chat.update(&key(KeyCode::Char('o')));
    let ChatIntent::Tool { intent, .. } = chat.take_intents().remove(0) else {
        panic!("details");
    };
    let mut c = call(State::Completed, "new output");
    c.revision = 1;
    assert_eq!(
        replace(&mut chat, Content::ToolCard(Box::new(c))),
        Updated::Applied
    );
    assert!(!chat.acknowledge_tool("reply", "tools", "run-1", &intent.ticket, Ok("")));
}
#[test]
fn chat_retry_is_atomic_new_history_and_foreign_or_duplicate_results_are_rejected() {
    let mut chat = chat(Content::ToolCard(Box::new(call(
        State::Failed,
        "original error",
    ))));
    chat_render(&mut chat, 100);
    chat.open_selection();
    chat.update(&key(KeyCode::Char('r')));
    let ChatIntent::Tool { intent, .. } = chat.take_intents().remove(0) else {
        panic!("retry");
    };
    let mut next = call(State::Running, "");
    assert!(!chat.complete_tool_retry("reply", "tools", "run-1", &intent.ticket, &next));
    next.id = "retry-1".into();
    assert!(chat.complete_tool_retry("reply", "tools", "run-1", &intent.ticket, &next));
    assert!(!chat.complete_tool_retry("reply", "tools", "run-1", &intent.ticket, &next));
    assert_eq!(chat.turn("reply").unwrap().parts.len(), 2);
    let text = chat_render(&mut chat, 100);
    assert!(text.contains("retried below") && !text.contains("original error"));
    chat.update(&key(KeyCode::Char('o')));
    let ChatIntent::Tool { intent, .. } = chat.take_intents().remove(0) else {
        panic!("retry selected");
    };
    assert_eq!(intent.call, "retry-1");
    assert_eq!(chat.input(), "and bump the version");
}
#[test]
fn group_selection_identity_survives_earlier_output_growth_and_resize() {
    let mut a = call(State::Running, "short");
    a.id = "a".into();
    let mut b = call(State::Running, "last");
    b.id = "b".into();
    let mut snapshot = ToolGroup {
        id: "group".into(),
        revision: 0,
        calls: vec![a, b],
    };
    let mut chat = chat(Content::ToolGroup(snapshot.clone()));
    chat_render(&mut chat, 100);
    chat.open_selection();
    snapshot.revision = 1;
    snapshot.calls[0].revision = 1;
    snapshot.calls[0].output = Output::Text("large output\n".repeat(1000));
    assert_eq!(
        replace(&mut chat, Content::ToolGroup(snapshot)),
        Updated::Applied
    );
    chat_render(&mut chat, 40);
    chat.update(&key(KeyCode::Char('o')));
    let ChatIntent::Tool { intent, .. } = chat.take_intents().remove(0) else {
        panic!("details");
    };
    assert_eq!(intent.call, "b");
}
#[test]
fn loaded_search_reveals_middle_of_capped_tool_output_then_restores_preview() {
    let mut output = "ordinary\n".repeat(40);
    output.push_str("unique needle\n");
    output.push_str(&"ordinary\n".repeat(40));
    let mut chat = chat(Content::ToolCard(Box::new(call(State::Completed, &output))));
    assert!(!chat_render(&mut chat, 100).contains("unique needle"));
    chat.open_search();
    chat.update(&Event::Paste("unique needle".into()));
    assert_eq!(chat.search_position(), (1, 1));
    assert!(chat_render(&mut chat, 100).contains("unique needle"));
    chat.close_search(false);
    assert!(!chat_render(&mut chat, 100).contains("unique needle"));
}
#[test]
fn invalid_tool_update_requests_a_usable_resync_and_message_budget_is_disclosed() {
    let mut chat = chat(Content::ToolCard(Box::new(call(State::Completed, ""))));
    let mut invalid = call(State::Running, "");
    invalid.revision = 1;
    let Updated::Resync(request) = replace(&mut chat, Content::ToolCard(Box::new(invalid))) else {
        panic!("resync");
    };
    let turn = chat.turn("reply").unwrap().clone();
    assert!(chat.resynchronize(&request, turn));
    let mut big = call(State::Completed, &"output\n".repeat(20_000));
    big.revision = 1;
    assert_eq!(
        replace(&mut chat, Content::ToolCard(Box::new(big))),
        Updated::Applied
    );
    assert!(chat_render(&mut chat, 100).contains("not retained"));
}
#[test]
fn tiny_areas_and_long_unicode_output_remain_bounded() {
    let c = call(State::Failed, &"界e\u{301}👩‍💻".repeat(5000));
    let card = ToolCallCard::new(c).unwrap();
    for width in [0, 1, 2, 20, 40, 100] {
        assert!(card.height(width, Symbols::Ascii) <= 258);
        let mut terminal = Terminal::new(TestBackend::new(width, 8)).unwrap();
        terminal
            .draw(|f| card.render_with(f, Rect::new(0, 0, width, 8), Presentation::default(), true))
            .unwrap();
    }
}

#[test]
fn extended_states_progress_and_control_markers_never_invent_results() {
    use canonic_tui::tool_call_card::Progress;
    for (state, expected) in [
        (State::Proposed, "not started"),
        (State::Cancelling, "cancelling"),
        (State::Interrupted("run interrupted".into()), "no result"),
    ] {
        let card = ToolCallCard::new(call(state, "")).expect("valid fixture");
        let text = render(&card, 100, Presentation::default(), false).0;
        assert!(text.contains(expected));
        assert!(!text.contains('✓'));
    }
    let mut c = call(State::Running, "");
    c.progress = Some(Progress {
        completed: 12,
        total: Some(40),
        unit: "files".into(),
    });
    let card = ToolCallCard::new(c.clone()).expect("valid progress");
    assert!(
        render(&card, 100, Presentation::default(), false)
            .0
            .contains("12 of 40 files")
    );
    c.progress.as_mut().unwrap().total = None;
    let card = ToolCallCard::new(c.clone()).expect("unknown total");
    let text = render(&card, 100, Presentation::default(), false).0;
    assert!(text.contains("12 files so far"));
    assert!(!text.contains('━'));
    c.result_unknown = true;
    let card = ToolCallCard::new(c).expect("unknown result");
    assert!(
        render(&card, 100, Presentation::default(), false)
            .0
            .contains("last seen running at 4s · result unknown")
    );
    let card = ToolCallCard::new(call(State::Failed, "\x1b[31mFAIL")).expect("safe control");
    let (_, buffer) = render(&card, 100, Presentation::default(), false);
    let marker = buffer
        .content
        .iter()
        .find(|cell| cell.symbol() == "␛")
        .unwrap();
    assert!(marker.modifier.contains(Modifier::DIM));
    let ascii = Presentation {
        symbols: Symbols::Ascii,
        ..Presentation::default()
    };
    assert!(render(&card, 100, ascii, false).0.contains("^[[31mFAIL"));
}

#[test]
fn child_task_and_artifact_effects_require_deliberate_enabled_commands() {
    use canonic_tui::tool_call_card::{Artifact, ChildTask};
    let mut c = call(State::Running, "");
    c.verb = "task".into();
    c.child_task = Some(ChildTask {
        id: "task-session".into(),
        latest_calls: vec![
            "old call".into(),
            "edit tests ✓".into(),
            "run tests ···".into(),
        ],
        total_calls: 3,
    });
    c.artifacts = vec![Artifact {
        id: "patch".into(),
        label: "settings.diff".into(),
        target: "fixture:patch".into(),
        open_available: false,
    }];
    let mut card = ToolCallCard::new(c.clone()).expect("valid child");
    let text = render(&card, 100, Presentation::default(), false).0;
    assert!(!text.contains("old call"));
    assert!(text.contains("edit tests"));
    assert!(card.hint().contains("o open task"));
    assert!(card.command(Command::OpenArtifact(0)).is_none());
    let open = card.command(Command::Details).unwrap();
    assert_eq!(
        open.effect,
        Effect::OpenTask {
            task: "task-session".into()
        }
    );
    assert!(card.acknowledge(&open.ticket, Ok("")));
    c.revision += 1;
    c.artifacts[0].open_available = true;
    assert!(card.replace(c));
    let open = card.command(Command::OpenArtifact(0)).unwrap();
    assert_eq!(
        open.effect,
        Effect::OpenArtifact {
            artifact: "patch".into(),
            target: "fixture:patch".into()
        }
    );
    assert!(card.command(Command::OpenArtifact(0)).is_none());
    assert!(card.acknowledge(&open.ticket, Err("host refused")));
    assert!(card.notice().contains("host refused"));
    assert!(!card.acknowledge(&open.ticket, Ok("late")));
}
