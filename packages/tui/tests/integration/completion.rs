//! Tests the standalone menu and its editor/source host boundaries.
use canonic_tui::{
    completion::{Action, Batch, Candidate, Menu},
    composer::{CompletionItem, CompletionKind, Composer, Intent},
    event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers},
    ratatui::{Terminal, backend::TestBackend},
    theme::{ColorMode, Presentation, Symbols},
};
use std::time::Duration;
fn key(code: KeyCode) -> Event {
    Event::Key(KeyEvent::new(code, KeyModifiers::NONE))
}
fn row(id: &str) -> Candidate {
    Candidate::new(id, id, id)
}
fn ready(rows: Vec<Candidate>, query: &str) -> Menu {
    let mut menu = Menu::new("files", query, &["repository"], rows.clone()).expect("valid fixture");
    assert!(menu.complete(
        &menu.request(),
        "repository",
        Batch {
            candidates: rows,
            complete: true,
            ..Batch::default()
        }
    ));
    menu
}
fn names(menu: &Menu) -> Vec<&str> {
    menu.matches().iter().map(|c| c.name.as_str()).collect()
}

#[test]
fn ranking_prefers_prefix_then_word_start_then_fuzzy_with_recency_ties() {
    let mut recent = row("setup");
    recent.recency = 4;
    let menu = ready(
        vec![row("s_e_t"), row("src/set"), row("settings"), recent],
        "set",
    );
    assert_eq!(names(&menu), ["setup", "settings", "src/set", "s_e_t"]);
    let menu = ready(vec![row("İstanbul"), row("alphabet")], "İs");
    assert_eq!(names(&menu), ["İstanbul"]);
}

#[test]
fn aliases_rank_after_name_words_and_before_recent_fuzzy_matches() {
    let mut alias = row("stop");
    alias.aliases = vec!["kill".into()];
    let mut fuzzy = row("k_i_l_l");
    fuzzy.recency = 100;
    let menu = ready(vec![fuzzy, alias, row("task kill"), row("killer")], "kill");
    assert_eq!(names(&menu), ["killer", "task kill", "stop", "k_i_l_l"]);
    let mut invalid = row("bad");
    invalid.aliases = vec!["alias".into(); 17];
    assert!(Menu::new("files", "", &["host"], vec![invalid]).is_err());
}

#[test]
fn timeout_is_a_warning_and_alt_retry_preserves_good_sources_and_rejects_repeat() {
    let mut menu = Menu::new("files", "", &["files"], Vec::new()).unwrap();
    let stale = menu.request();
    menu.tick(Duration::from_secs(5));
    assert_eq!(menu.timed_out_sources(), ["files"]);
    assert_eq!(menu.failures(), [] as [(&str, &str); 0]);
    let mut terminal = Terminal::new(TestBackend::new(80, 12)).unwrap();
    terminal
        .draw(|f| menu.render(f, f.area(), Presentation::default()))
        .unwrap();
    let text = super::components::dump(terminal.backend().buffer());
    assert!(text.contains("! no reply from files after 5 s"));
    assert!(text.contains("slow or stuck"));
    let alt = KeyEvent::new(KeyCode::Char('r'), KeyModifiers::ALT);
    let Action::Retry(retry) = menu.update(&Event::Key(alt)) else {
        panic!("retry")
    };
    assert_ne!(retry, stale);
    assert!(!menu.complete(&stale, "files", Batch::default()));
    assert_eq!(menu.timed_out_sources(), [] as [&str; 0]);
    assert_eq!(menu.pending_sources(), ["files"]);
    let mut menu = Menu::new("commands", "", &["good", "bad"], Vec::new()).unwrap();
    let request = menu.request();
    menu.complete(
        &request,
        "good",
        Batch {
            candidates: vec![row("usable")],
            complete: true,
            ..Batch::default()
        },
    );
    menu.complete(
        &request,
        "bad",
        Batch {
            error: Some("offline".into()),
            ..Batch::default()
        },
    );
    assert!(matches!(menu.update(&key(KeyCode::Tab)), Action::Insert(c) if c.id == "usable"));
    for kind in [KeyEventKind::Repeat, KeyEventKind::Release] {
        assert_eq!(
            menu.update(&Event::Key(KeyEvent { kind, ..alt })),
            Action::None
        );
        assert_eq!(menu.failures(), [("bad", "offline")]);
    }
    assert!(matches!(menu.update(&Event::Key(alt)), Action::Retry(_)));
    assert_eq!(menu.pending_sources(), ["bad"]);
    assert_eq!(menu.selected().unwrap().id, "usable");
    assert_eq!(names(&menu), ["usable"]);
}

#[test]
fn composer_alt_retry_and_control_history_are_distinct_routes_with_separate_hints() {
    let mut composer = composer(256);
    assert!(composer.set_history(vec!["previous message".into()]));
    type_text(&mut composer, "/");
    let request = composer.completion_request().unwrap();
    assert!(composer.set_completion_sources(&request, &["good", "bad"]));
    assert!(composer.complete_source(
        &request,
        "good",
        Batch {
            candidates: vec![Candidate::new("review", "/review", "review")],
            complete: true,
            ..Batch::default()
        }
    ));
    assert!(composer.complete_source(
        &request,
        "bad",
        Batch {
            error: Some("offline".into()),
            ..Batch::default()
        }
    ));
    assert!(
        composer
            .completion_hint_for(80)
            .unwrap()
            .contains("alt+r retries")
    );
    assert!(!composer.footer_hint_for(80).contains("inserts"));
    let cursor = composer.editor().cursor();
    composer.update(&Event::Key(KeyEvent::new(
        KeyCode::Char('r'),
        KeyModifiers::ALT,
    )));
    assert!(matches!(
        composer.take_intents().as_slice(),
        [Intent::RetryCompletion(_)]
    ));
    assert_eq!(composer.editor().cursor(), cursor);
    assert_eq!(composer.editor().text(), "/");
    composer.update(&Event::Key(KeyEvent::new(
        KeyCode::Char('r'),
        KeyModifiers::CONTROL,
    )));
    assert!(composer.completion_menu().is_none());
    assert!(composer.take_intents().is_empty());
    assert!(composer.footer_hint().contains("enter recalls"));
    composer.update(&key(KeyCode::Esc));
    assert_eq!(composer.editor().text(), "/");
    assert_eq!(composer.editor().cursor(), cursor);
}

#[test]
fn no_match_names_the_route_and_f1_opens_help_without_submission() {
    let mut menu = Menu::new("commands", "depoy", &["host"], Vec::new()).unwrap();
    menu.complete(
        &menu.request(),
        "host",
        Batch {
            complete: true,
            ..Batch::default()
        },
    );
    let mut terminal = Terminal::new(TestBackend::new(80, 12)).unwrap();
    terminal
        .draw(|f| menu.render(f, f.area(), Presentation::default()))
        .unwrap();
    let text = super::components::dump(terminal.backend().buffer());
    assert!(text.contains("no command matches \"/depoy\""));
    assert!(text.contains("F1 lists every key"));
    let mut composer = composer(256);
    type_text(&mut composer, "/depoy");
    let cursor = composer.editor().cursor();
    assert!(composer.update(&key(KeyCode::F(1))).is_none());
    assert!(composer.completion_menu().is_none());
    assert!(composer.footer_hint().contains("type filters"));
    composer.update(&key(KeyCode::Esc));
    assert_eq!(composer.editor().text(), "/depoy");
    assert_eq!(composer.editor().cursor(), cursor);
    assert!(composer.take_intents().is_empty());
}

#[test]
fn narrow_disabled_reason_and_whole_grapheme_clipping_preserve_full_payloads() {
    use canonic_tui::ratatui::style::Color;
    use unicode_width::UnicodeWidthStr;
    let mut disabled = row("/resume");
    disabled.disabled = Some("needs network".into());
    disabled.description = "description must yield to availability".into();
    let mut menu = ready(vec![row("/review"), disabled], "");
    let mut terminal = Terminal::new(TestBackend::new(40, 12)).unwrap();
    terminal
        .draw(|f| {
            menu.render(
                f,
                f.area(),
                Presentation {
                    colors: ColorMode::NoColor,
                    ..Presentation::default()
                },
            );
        })
        .unwrap();
    assert!(super::components::dump(terminal.backend().buffer()).contains("[-] needs network"));
    assert!(
        terminal
            .backend()
            .buffer()
            .content
            .iter()
            .all(|cell| cell.fg == Color::Reset && cell.bg == Color::Reset)
    );
    let path = "docs/設計/e\u{301}🦀/タイムゾーン設定.md";
    let mut menu = ready(vec![Candidate::new("wide", path, path)], "設計");
    let mut terminal = Terminal::new(TestBackend::new(20, 8)).unwrap();
    terminal
        .draw(|f| menu.render(f, f.area(), Presentation::default()))
        .unwrap();
    let buffer = terminal.backend().buffer();
    let mut line = String::new();
    let mut x = 0;
    while x < 20 {
        let symbol = buffer[(x, 6)].symbol();
        line.push_str(symbol);
        x += u16::try_from(symbol.width().max(1)).unwrap();
    }
    assert!(line.contains("docs/設計/e\u{301}🦀/タ⋯"), "{line:?}");
    assert!(matches!(menu.update(&key(KeyCode::Tab)), Action::Insert(c) if c.insertion == path));
}

#[test]
fn removed_command_refuses_once_then_literal_enter_cannot_invoke_a_duplicate_label() {
    let mut composer = composer(256);
    type_text(&mut composer, "/rev");
    composer.update(&key(KeyCode::Tab));
    assert!(composer.set_completions(vec![CompletionItem {
        id: "different-command".into(),
        value: "review".into(),
        detail: String::new(),
        kind: CompletionKind::Command,
        disabled: None,
        touched: false,
    }]));
    assert!(composer.update(&key(KeyCode::Enter)).is_none());
    assert_eq!(composer.editor().text(), "/review ");
    assert!(composer.take_intents().is_empty());
    assert!(composer.footer_hint().contains("enter sends as text"));
    let submission = composer
        .update(&key(KeyCode::Enter))
        .expect("explicit literal submission");
    assert_eq!(submission.text, "/review ");
    assert!(!composer.take_intents().iter().any(|intent| matches!(
        intent,
        Intent::Command(_) | Intent::CommandWithArguments { .. }
    )));
}

#[test]
fn navigation_skips_disabled_without_wrap_and_repeats_never_confirm() {
    let mut disabled = row("b");
    disabled.disabled = Some("offline".into());
    let mut menu = ready(vec![row("a"), disabled, row("c")], "");
    menu.update(&key(KeyCode::Up));
    assert_eq!(menu.selected().unwrap().id, "a");
    menu.update(&key(KeyCode::Down));
    assert_eq!(menu.selected().unwrap().id, "c");
    menu.update(&key(KeyCode::Down));
    assert_eq!(menu.selected().unwrap().id, "c");
    for kind in [KeyEventKind::Repeat, KeyEventKind::Release] {
        let mut event = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
        event.kind = kind;
        assert_eq!(menu.update(&Event::Key(event)), Action::None);
    }
    assert!(matches!(menu.update(&key(KeyCode::Enter)),Action::Insert(c) if c.id=="c"));
    menu.close();
    assert_eq!(menu.update(&key(KeyCode::Tab)), Action::Unhandled);
}

#[test]
fn streaming_preserves_identity_order_selection_and_rejects_finished_foreign_stale_batches() {
    let mut menu = Menu::new("files", "", &["one", "two"], Vec::new()).unwrap();
    let req = menu.request();
    assert!(menu.complete(
        &req,
        "one",
        Batch {
            candidates: vec![row("b"), row("c")],
            ..Batch::default()
        }
    ));
    menu.update(&key(KeyCode::Down));
    assert!(menu.complete(
        &req,
        "one",
        Batch {
            candidates: vec![row("a"), row("c"), row("b")],
            complete: true,
            ..Batch::default()
        }
    ));
    assert_eq!(names(&menu), ["b", "c", "a"]);
    assert_eq!(menu.selected().unwrap().id, "c");
    assert!(!menu.complete(&req, "one", Batch::default()));
    let foreign = Menu::new("files", "", &["one"], Vec::new())
        .unwrap()
        .request();
    assert!(!menu.complete(&foreign, "two", Batch::default()));
    menu.set_query("a");
    assert!(!menu.complete(&req, "two", Batch::default()));
    let current = menu.request();
    menu.close();
    assert!(!menu.complete(&current, "two", Batch::default()));
}

#[test]
fn injected_clock_debounces_shows_loading_and_times_out_without_results_then_retries() {
    let mut menu = Menu::new("files", "", &["repository"], Vec::new()).unwrap();
    let stale = menu.request();
    menu.tick(Duration::from_millis(59));
    assert!(menu.take_request().is_none());
    menu.set_query("s");
    menu.tick(Duration::from_millis(60));
    assert!(menu.take_request().is_some());
    assert!(menu.take_request().is_none());
    assert!(!menu.loading());
    menu.tick(Duration::from_millis(90));
    assert!(menu.loading());
    assert!(!menu.complete(&stale, "repository", Batch::default()));
    menu.tick(Duration::from_secs(5));
    assert!(!menu.loading());
    assert!(menu.hint().contains("retries"));
    let Action::Retry(retry) = menu.update(&key(KeyCode::Tab)) else {
        panic!("expected retry")
    };
    assert_eq!(menu.pending_sources(), ["repository"]);
    assert!(menu.complete(
        &retry,
        "repository",
        Batch {
            candidates: vec![row("settings")],
            complete: true,
            ..Batch::default()
        }
    ));
    assert_eq!(menu.failures(), [] as [(&str, &str); 0]);
}

#[test]
fn partial_failure_keeps_results_and_guess_only_tab_inserts() {
    let mut menu = Menu::new("files", "", &["one", "two"], Vec::new()).unwrap();
    let req = menu.request();
    menu.complete(
        &req,
        "one",
        Batch {
            candidates: vec![row("a")],
            complete: true,
            ..Batch::default()
        },
    );
    menu.complete(
        &req,
        "two",
        Batch {
            error: Some("offline".into()),
            ..Batch::default()
        },
    );
    assert_eq!(menu.failures(), [("two", "offline")]);
    assert!(matches!(menu.update(&key(KeyCode::Tab)),Action::Insert(c) if c.id=="a"));
    let mut menu = ready(Vec::new(), "typo");
    assert!(menu.set_guess(&menu.request(), row("closest")));
    assert_eq!(menu.update(&key(KeyCode::Enter)), Action::PassEnter);
    assert!(matches!(menu.update(&key(KeyCode::Tab)),Action::Insert(c) if c.id=="closest"));
}

#[test]
fn invalid_source_updates_are_atomic_and_bounded() {
    let mut menu = Menu::new("files", "", &["one", "two"], Vec::new()).unwrap();
    let req = menu.request();
    menu.complete(
        &req,
        "one",
        Batch {
            candidates: vec![row("a")],
            complete: true,
            ..Batch::default()
        },
    );
    for rows in [
        vec![row("a")],
        vec![row("dup"), row("dup")],
        vec![Candidate::new("control", "control", "\x1b[31m")],
        (0..5001).map(|i| row(&i.to_string())).collect(),
    ] {
        assert!(!menu.complete(
            &req,
            "two",
            Batch {
                candidates: rows,
                complete: true,
                ..Batch::default()
            }
        ));
        assert_eq!(names(&menu), ["a"]);
        assert_eq!(menu.pending_sources(), ["two"]);
    }
}
fn composer(limit: usize) -> Composer {
    let mut c = Composer::new(limit);
    c.set_completions(vec![
        CompletionItem {
            id: "review".into(),
            value: "review".into(),
            detail: "review a pull request".into(),
            kind: CompletionKind::Command,
            disabled: None,
            touched: false,
        },
        CompletionItem {
            id: "file".into(),
            value: "settings.ts".into(),
            detail: String::new(),
            kind: CompletionKind::File,
            disabled: None,
            touched: false,
        },
    ]);
    c
}
fn type_text(c: &mut Composer, text: &str) {
    for ch in text.chars() {
        assert!(c.update(&key(KeyCode::Char(ch))).is_none());
    }
}

#[test]
fn command_acceptance_inserts_then_later_enter_invokes_and_no_match_sends_literal() {
    for accept in [KeyCode::Tab, KeyCode::Enter] {
        let mut c = composer(256);
        type_text(&mut c, "/rev");
        assert!(c.update(&key(accept)).is_none());
        assert_eq!(c.editor().text(), "/review ");
        assert!(c.take_intents().is_empty());
        assert!(c.update(&key(KeyCode::Enter)).is_none());
        assert!(matches!(c.take_intents().as_slice(),[Intent::Command(id)] if id=="review"));
        assert_eq!(c.editor().text(), "");
    }
    let mut c = composer(256);
    type_text(&mut c, "/unknown");
    let submission = c.update(&key(KeyCode::Enter)).unwrap();
    assert_eq!(submission.text, "/unknown");
    assert_eq!(c.editor().text(), "");
    let mut c = composer(256);
    type_text(&mut c, "/review");
    let req = c.completion_request().unwrap();
    c.set_completion_status(
        &req,
        canonic_tui::picker::Status::Error("Source failed".into()),
    );
    let submission = c.update(&key(KeyCode::Enter)).unwrap();
    assert_eq!(submission.text, "/review");
    assert!(c.take_intents().is_empty());
}

#[test]
fn cursor_inside_token_keeps_full_query_replaces_suffix_and_no_fit_keeps_menu() {
    let mut existing = composer(256);
    existing.set_text("set tail");
    existing.update(&key(KeyCode::Home));
    existing.update(&key(KeyCode::Char('@')));
    assert_eq!(existing.completion_request().unwrap().query, "set");
    existing.update(&key(KeyCode::Right));
    assert!(existing.overlay_open());
    existing.update(&key(KeyCode::Tab));
    assert_eq!(existing.editor().expanded_text(), "@settings.ts tail");
    let mut c = composer(256);
    type_text(&mut c, "@set");
    let req = c.completion_request().unwrap();
    c.update(&key(KeyCode::Left));
    assert_eq!(c.completion_request().unwrap(), req);
    c.update(&Event::Key(KeyEvent::new(
        KeyCode::Char('e'),
        KeyModifiers::CONTROL,
    )));
    assert_eq!(c.completion_request().unwrap(), req);
    c.update(&key(KeyCode::Left));
    c.update(&key(KeyCode::Tab));
    assert_eq!(c.editor().expanded_text(), "@settings.ts ");
    let mut c = composer(4);
    type_text(&mut c, "@set");
    c.update(&key(KeyCode::Left));
    let cursor = c.editor().cursor();
    c.update(&key(KeyCode::Tab));
    assert_eq!(c.editor().text(), "@set");
    assert_eq!(c.editor().cursor(), cursor);
    assert!(c.overlay_open());
}

#[test]
fn dismiss_paste_new_trigger_and_shell_paths_respect_host_token_rules() {
    let mut c = composer(256);
    type_text(&mut c, "@s");
    c.update(&key(KeyCode::Esc));
    type_text(&mut c, "et");
    assert!(!c.overlay_open());
    type_text(&mut c, " @");
    assert!(c.overlay_open());
    let draft = c.editor().text().to_owned();
    c.update(&Event::Key(KeyEvent::new(
        KeyCode::Char('c'),
        KeyModifiers::CONTROL,
    )));
    assert_eq!(c.editor().text(), draft);
    assert!(!c.overlay_open());
    type_text(&mut c, " @");
    c.update(&Event::Paste("/review".into()));
    assert!(!c.overlay_open());
    c.set_text("");
    c.update(&Event::Paste("/review".into()));
    assert!(!c.overlay_open());
    c.set_text("");
    type_text(&mut c, "!cat ./set");
    assert!(c.overlay_open());
    c.update(&key(KeyCode::Home));
    assert!(!c.overlay_open());
}

#[test]
fn fixed_height_details_pages_unicode_and_profiles_render_without_focus_changes() {
    let mut rows: Vec<_> = (0..20).map(|i| row(&format!("file-{i:02}"))).collect();
    rows[8].name = "界e\u{301}🦀".into();
    rows[8].description = "long description ".repeat(20);
    let mut menu = ready(rows, "");
    let opening = menu.height(80);
    menu.set_query("file");
    assert_eq!(menu.height(80), opening);
    menu.update(&key(KeyCode::PageDown));
    for colors in [
        ColorMode::TrueColor,
        ColorMode::Ansi256,
        ColorMode::Ansi16,
        ColorMode::NoColor,
    ] {
        for width in [0, 1, 20, 29, 30, 40, 80] {
            for height in [0, 1, 2, 4, 12] {
                let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
                terminal
                    .draw(|f| {
                        menu.render(
                            f,
                            f.area(),
                            Presentation {
                                colors,
                                symbols: Symbols::Ascii,
                                ..Presentation::default()
                            },
                        );
                    })
                    .unwrap();
            }
        }
    }
    assert_eq!(menu.request().query, "file");
    assert_eq!(menu.height(80), opening);
    assert!(menu.height(29) <= 7);
}

proptest::proptest! {
    #[test]
    fn arbitrary_unicode_queries_keep_selection_in_the_matching_set(query in ".{0,80}") {
        let mut menu=ready(vec![row("界e\u{301}"),row("settings"),row("src/file.rs")],"");
        menu.set_query(&query);
        for code in [KeyCode::Up,KeyCode::PageDown,KeyCode::PageUp,KeyCode::Down] {menu.update(&key(code));}
        if let Some(selected)=menu.selected() {proptest::prop_assert!(menu.matches().iter().any(|c| c.id==selected.id));}
    }
}

#[test]
fn selected_command_identity_survives_duplicate_labels_and_rechecks_availability() {
    let mut c = composer(256);
    type_text(&mut c, "/");
    let req = c.completion_request().unwrap();
    c.set_completion_sources(&req, &["plugin"]);
    c.complete_source(
        &req,
        "plugin",
        Batch {
            candidates: vec![
                Candidate::new("first", "/review", "review"),
                Candidate::new("second", "/review", "review"),
            ],
            complete: true,
            ..Batch::default()
        },
    );
    c.update(&key(KeyCode::Down));
    c.update(&key(KeyCode::Enter));
    type_text(&mut c, "123");
    c.update(&key(KeyCode::Enter));
    assert!(
        matches!(c.take_intents().as_slice(),[Intent::CommandWithArguments {id,arguments}] if id=="second" && arguments=="123")
    );
    type_text(&mut c, "/review");
    c.update(&key(KeyCode::Tab));
    c.set_completions(vec![CompletionItem {
        id: "review".into(),
        value: "review".into(),
        detail: String::new(),
        kind: CompletionKind::Command,
        disabled: Some("offline".into()),
        touched: false,
    }]);
    assert!(c.update(&key(KeyCode::Enter)).is_none());
    assert_eq!(c.editor().text(), "/review ");
    assert!(c.take_intents().is_empty());
    assert!(c.status().contains("unavailable"));
}

#[test]
fn asynchronous_command_insertion_preserves_arguments_and_visual_hints_are_not_payload() {
    let mut c = composer(256);
    type_text(&mut c, "/");
    let req = c.completion_request().unwrap();
    assert!(c.set_completion_sources(&req, &["plugin"]));
    let mut item = Candidate::new("plugin-review", "/audit", "audit");
    item.argument_hint = "/audit takes a target".into();
    item.argument_placeholder = "<target>".into();
    assert!(c.complete_source(
        &req,
        "plugin",
        Batch {
            candidates: vec![item],
            complete: true,
            ..Batch::default()
        }
    ));
    c.update(&key(KeyCode::Enter));
    assert_eq!(c.editor().text(), "/audit ");
    assert_eq!(c.status(), "/audit takes a target");
    let mut terminal = Terminal::new(TestBackend::new(64, 5)).unwrap();
    terminal
        .draw(|f| {
            c.render_with(
                f,
                f.area(),
                canonic_tui::editor::EditorView::default(),
                true,
            );
        })
        .unwrap();
    assert!(super::components::dump(terminal.backend().buffer()).contains("/audit <target>"));
    assert_eq!(c.editor().expanded_text(), "/audit ");
    type_text(&mut c, "project");
    assert!(c.footer_hint().contains("enter runs command"));
    c.update(&key(KeyCode::Enter));
    assert!(
        matches!(c.take_intents().as_slice(),[Intent::CommandWithArguments {id,arguments}] if id=="plugin-review" && arguments=="project")
    );
}

#[test]
fn partial_retry_keeps_good_sources_and_invalidates_old_requests() {
    let mut menu = Menu::new("files", "", &["one", "two"], Vec::new()).unwrap();
    let req = menu.request();
    menu.complete(
        &req,
        "one",
        Batch {
            candidates: vec![row("a")],
            complete: true,
            ..Batch::default()
        },
    );
    menu.complete(
        &req,
        "two",
        Batch {
            error: Some("offline".into()),
            ..Batch::default()
        },
    );
    let retry = menu.retry_failed().unwrap();
    assert_eq!(names(&menu), ["a"]);
    assert_eq!(menu.pending_sources(), ["two"]);
    assert!(!menu.complete(&req, "two", Batch::default()));
    menu.complete(
        &retry,
        "two",
        Batch {
            candidates: vec![row("b")],
            complete: true,
            ..Batch::default()
        },
    );
    assert_eq!(names(&menu), ["a", "b"]);
}

#[test]
fn legacy_timeout_rejects_the_late_result_and_retry_accepts_only_the_new_ticket() {
    let mut c = composer(256);
    type_text(&mut c, "@");
    let req = c.completion_request().unwrap();
    c.set_completion_status(
        &req,
        canonic_tui::picker::Status::Loading("Searching".into()),
    );
    c.tick(Duration::from_secs(5));
    assert!(!c.complete(&req, Ok(vec![])));
    c.update(&key(KeyCode::Tab));
    let retry = c.completion_request().unwrap();
    assert_ne!(req.ticket, retry.ticket);
    assert!(!c.complete(&req, Ok(vec![])));
    assert!(c.complete(&retry, Ok(vec![])));
}

#[test]
fn host_replacement_and_lock_invalidate_completion_routes() {
    let mut c = composer(256);
    type_text(&mut c, "@set");
    let req = c.completion_request().unwrap();
    c.set_text("replacement");
    assert!(!c.overlay_open());
    assert!(!c.complete(&req, Ok(vec![])));
    c.set_text("");
    type_text(&mut c, "@");
    assert!(c.lock("editor"));
    assert!(!c.overlay_open());
    c.update(&key(KeyCode::Char('x')));
    assert_eq!(c.editor().text(), "@");
}
