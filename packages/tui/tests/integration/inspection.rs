//! Public inspection contracts: exact host effects, navigation, folds and safe rendering.
use canonic_tui::{
    event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers},
    inspection::{Content, Inspector, Intent, Invalid, Item, Operation, Origin, Outcome, State},
    ratatui::{
        Terminal,
        backend::TestBackend,
        layout::Rect,
        style::{Color, Modifier},
    },
    theme::{ColorMode, Presentation, Symbols, Theme},
};
fn key(code: KeyCode) -> Event {
    Event::Key(KeyEvent::new(code, KeyModifiers::NONE))
}
fn code(text: &str) -> Inspector {
    Inspector::new(
        Item::new("file", "src/settings.ts"),
        State::Ready(Content::Code(text.into())),
    )
    .expect("valid inspection fixture or result")
}
fn patch() -> Inspector {
    Inspector::new(Item::new("patch","src/settings.ts"),State::Ready(Content::Diff {patch:"@@ -1,3 +1,4 @@ settings\n first\n-old tz\n+new tz\n+timezone\n last\n@@ -8 +9 @@ send\n-end\n+done\n".into(),new_file:Some("first\nnew tz\ntimezone\nlast\ngap\ngap\ngap\ngap\ndone".into())})).expect("valid inspection fixture or result")
}
fn screen(v: &mut Inspector, w: u16, h: u16, p: Presentation) -> String {
    let mut terminal =
        Terminal::new(TestBackend::new(w, h)).expect("valid inspection fixture or result");
    terminal
        .draw(|f| v.render(f, f.area(), p))
        .expect("valid inspection fixture or result");
    super::components::dump(terminal.backend().buffer())
}
fn request(v: &mut Inspector) -> canonic_tui::inspection::Request {
    let intents = v.take_intents();
    let Intent::Request(r) = intents
        .into_iter()
        .next()
        .expect("valid inspection fixture or result")
    else {
        panic!("request")
    };
    r
}
#[test]
fn code_navigation_line_fields_paste_and_layered_escape_are_read_only() {
    let mut v = code("one\ntwo\nthree\nfour");
    v.update(&key(KeyCode::Down));
    assert_eq!(v.line(), Some(2));
    v.update(&Event::Paste("e:y/\n".into()));
    assert_eq!(v.line(), Some(2));
    assert_eq!(v.take_intents(), Vec::<Intent>::new());
    v.update(&key(KeyCode::Char(':')));
    v.update(&Event::Paste("9999".into()));
    v.update(&key(KeyCode::Enter));
    assert_eq!(v.line(), Some(4));
    v.update(&key(KeyCode::Char('/')));
    v.update(&Event::Paste("TWO".into()));
    assert_eq!(v.match_count(), 0);
    v.update(&key(KeyCode::Esc));
    v.update(&key(KeyCode::Char('/')));
    v.update(&Event::Paste("two".into()));
    assert_eq!(v.match_count(), 1);
    v.update(&key(KeyCode::Enter));
    assert_eq!(v.line(), Some(2));
    v.update(&key(KeyCode::Char('v')));
    v.update(&key(KeyCode::Down));
    v.update(&key(KeyCode::Esc));
    assert!(v.active());
    assert!(v.selection().is_none());
    assert_eq!(v.match_count(), 1);
    v.update(&key(KeyCode::Esc));
    assert!(v.active());
    assert_eq!(v.match_count(), 0);
    v.update(&key(KeyCode::Esc));
    assert!(!v.active());
    assert_eq!(v.take_intents(), vec![Intent::Returned]);
    v.reopen(Origin::Chat);
    assert_eq!(v.line(), Some(3));
}
#[test]
fn diff_hunks_whole_file_goto_and_new_code_copy_preserve_numbering() {
    let mut v = patch();
    v.update(&key(KeyCode::Char(']')));
    assert!(screen(&mut v, 90, 20, Presentation::default()).contains("hunk 2 of 2"));
    v.update(&key(KeyCode::Char(']')));
    assert_eq!(v.notice(), "last hunk");
    v.update(&key(KeyCode::Char('[')));
    v.update(&key(KeyCode::Char(':')));
    v.update(&Event::Paste("2".into()));
    v.update(&key(KeyCode::Enter));
    assert_eq!(v.line(), Some(2));
    v.update(&key(KeyCode::Char('v')));
    v.update(&key(KeyCode::Down));
    v.update(&key(KeyCode::Char('c')));
    let r = request(&mut v);
    let Operation::Copy { text, lines, patch } = &r.operation else {
        panic!("copy")
    };
    assert_eq!(text, "new tz\ntimezone\n");
    assert_eq!(*lines, 2);
    assert!(!patch);
    assert!(
        v.complete(&r, Outcome::Copied)
            .expect("valid inspection fixture or result")
    );
    assert!(v.selection().is_none());
    assert!(
        !v.complete(&r, Outcome::Copied)
            .expect("valid inspection fixture or result")
    );
    v.update(&key(KeyCode::Tab));
    assert!(v.whole_file());
    assert_eq!(v.line(), Some(3));
    v.update(&key(KeyCode::Tab));
    assert!(!v.whole_file());
    assert_eq!(v.line(), Some(3));
}
#[test]
fn folded_content_is_searchable_and_selection_copies_hidden_lines() {
    let content = format!(
        "@@ -1,20 +1,20 @@ folded\n{}",
        (1..=20)
            .map(|i| format!(" line-{i}"))
            .collect::<Vec<_>>()
            .join("\n")
    );
    let mut v = Inspector::new(
        Item::new("fold", "fold.ts"),
        State::Ready(Content::Diff {
            patch: content,
            new_file: None,
        }),
    )
    .expect("valid inspection fixture or result");
    assert!(screen(&mut v, 80, 24, Presentation::default()).contains("14 unchanged lines"));
    for _ in 0..4 {
        v.update(&key(KeyCode::Down));
    }
    v.update(&key(KeyCode::Char('v')));
    v.update(&key(KeyCode::Down));
    v.update(&key(KeyCode::Char('y')));
    let r = request(&mut v);
    let Operation::Copy { text, .. } = &r.operation else {
        panic!("copy")
    };
    assert!(text.contains(" line-4\n"));
    assert!(text.contains(" line-17\n"));
    v.complete(&r, Outcome::Failed("no clipboard".into()))
        .expect("valid inspection fixture or result");
    assert!(v.selection().is_some());
    v.update(&key(KeyCode::Esc));
    v.update(&key(KeyCode::Char('/')));
    v.update(&Event::Paste("line-12".into()));
    assert_eq!(v.match_count(), 1);
    v.update(&key(KeyCode::Enter));
    assert_eq!(v.line(), Some(12));
    assert!(!screen(&mut v, 80, 24, Presentation::default()).contains("14 unchanged lines"));
}
#[test]
fn loading_retry_revision_close_and_foreign_results_are_exact_and_atomic() {
    let mut item = Item::new("file", "file.rs");
    item.editor = true;
    let mut v = Inspector::new(item.clone(), State::Failed("denied".into()))
        .expect("valid inspection fixture or result");
    v.update(&key(KeyCode::Char('r')));
    let r = request(&mut v);
    assert!(matches!(r.operation, Operation::Load));
    assert!(
        !v.complete(&r, Outcome::Copied)
            .expect("valid inspection fixture or result")
    );
    assert!(v.pending().is_some());
    assert_eq!(
        v.complete(
            &r,
            Outcome::Loaded(State::Ready(Content::Diff {
                patch: "@@ broken".into(),
                new_file: None
            }))
        ),
        Err(Invalid::Patch)
    );
    assert!(v.pending().is_some());
    assert!(
        v.complete(
            &r,
            Outcome::Loaded(State::Ready(Content::Code("loaded".into())))
        )
        .expect("valid inspection fixture or result")
    );
    assert_eq!(v.line(), Some(1));
    v.update(&key(KeyCode::Char('e')));
    let editor = request(&mut v);
    v.update(&key(KeyCode::Esc));
    v.reopen(Origin::Chat);
    assert!(
        !v.complete(&editor, Outcome::Opened)
            .expect("valid inspection fixture or result")
    );
    v.update(&key(KeyCode::Char('e')));
    let current = request(&mut v);
    let mut other = Inspector::new(item.clone(), State::Ready(Content::Code("loaded".into())))
        .expect("valid inspection fixture or result");
    other.update(&key(KeyCode::Char('e')));
    let foreign = request(&mut other);
    assert!(
        !v.complete(&foreign, Outcome::Opened)
            .expect("valid inspection fixture or result")
    );
    item.revision += 1;
    v.replace(item.clone(), State::Ready(Content::Code("new".into())))
        .expect("valid inspection fixture or result");
    assert!(
        !v.complete(&current, Outcome::Opened)
            .expect("valid inspection fixture or result")
    );
    assert_eq!(
        v.replace(item, State::Ready(Content::Code("bad".into()))),
        Err(Invalid::Revision)
    );
    assert!(screen(&mut v, 80, 10, Presentation::default()).contains("new"));
}
#[test]
fn partial_binary_deleted_malformed_and_bounded_content_have_explicit_states() {
    assert!(matches!(
        Inspector::new(
            Item::new("file", "file"),
            State::Ready(Content::Code("x".repeat(65_537)))
        ),
        Err(Invalid::Limit)
    ));
    for patch in [
        "@@ -0 +1 @@\n+x",
        "@@ -1,2 +1,2 @@\n one",
        "@@ -1 +1 @@\n-old\n+new\n@@ -1 +1 @@\n-old\n+new",
        "--- a/file\n+++ b/file\n@@ -1 +1 @@\n-old\n+wrong",
    ] {
        let new_file = patch.contains("wrong").then(|| "new".into());
        assert!(
            Inspector::new(
                Item::new("patch", "patch"),
                State::Ready(Content::Diff {
                    patch: patch.into(),
                    new_file
                })
            )
            .is_err(),
            "{patch}"
        );
    }
    let mut v = Inspector::new(
        Item::new("file", "large"),
        State::Unavailable {
            reason: "over host limit".into(),
            preview: Some(Content::Code("bounded part".into())),
        },
    )
    .expect("valid inspection fixture or result");
    assert!(screen(&mut v, 80, 12, Presentation::default()).contains("over host limit"));
    v.update(&key(KeyCode::Enter));
    assert!(screen(&mut v, 80, 12, Presentation::default()).contains("partial content"));
    let mut deleted = Inspector::new(
        Item::new("deleted", "deleted"),
        State::Ready(Content::Diff {
            patch: "@@ -1 +0,0 @@\n-removed".into(),
            new_file: None,
        }),
    )
    .expect("valid inspection fixture or result");
    deleted.update(&key(KeyCode::Down));
    deleted.update(&key(KeyCode::Char('v')));
    deleted.update(&key(KeyCode::Char('c')));
    assert_eq!(deleted.take_intents(), Vec::<Intent>::new());
    assert_eq!(deleted.notice(), "selection has no new code");
    deleted.update(&key(KeyCode::Tab));
    assert!(deleted.notice().contains("not supplied"));
}
#[test]
fn repeat_release_modified_effects_and_unfocused_input_cannot_issue_work() {
    let mut v = code("line");
    v.update(&key(KeyCode::Char('v')));
    for kind in [KeyEventKind::Repeat, KeyEventKind::Release] {
        for code in [
            KeyCode::Char('y'),
            KeyCode::Char('c'),
            KeyCode::Char('e'),
            KeyCode::Esc,
            KeyCode::Tab,
        ] {
            let mut k = KeyEvent::new(code, KeyModifiers::NONE);
            k.kind = kind;
            v.update(&Event::Key(k));
        }
    }
    assert_eq!(v.take_intents(), Vec::<Intent>::new());
    assert!(v.active());
    v.set_focused(false);
    assert!(!v.update(&key(KeyCode::Char('y'))));
    assert_eq!(v.take_intents(), Vec::<Intent>::new());
}
#[test]
fn rendering_profiles_tiny_unicode_controls_and_subrect_are_safe() {
    for theme in [Theme::Dark, Theme::Light] {
        for colors in [
            ColorMode::NoColor,
            ColorMode::Ansi16,
            ColorMode::Ansi256,
            ColorMode::TrueColor,
        ] {
            for symbols in [Symbols::Ascii, Symbols::Unicode] {
                let p = Presentation {
                    theme,
                    colors,
                    symbols,
                };
                let mut v = code("東京 👩‍💻 e\u{301}\t\x1b[31m\x07");
                for (w, h) in [(0, 0), (1, 1), (10, 2), (20, 3), (40, 8), (80, 20)] {
                    let text = screen(&mut v, w, h, p);
                    assert!(!text.contains('\x1b'));
                    assert!(!text.contains('\x07'));
                }
                let mut t = Terminal::new(TestBackend::new(90, 20))
                    .expect("valid inspection fixture or result");
                t.draw(|f| v.render(f, Rect::new(5, 3, 50, 10), p))
                    .expect("valid inspection fixture or result");
                assert_eq!(t.backend().buffer()[(0, 0)].symbol(), " ");
                if colors == ColorMode::NoColor {
                    assert!(
                        t.backend()
                            .buffer()
                            .content
                            .iter()
                            .all(|c| c.fg == Color::Reset && c.bg == Color::Reset)
                    );
                    assert!(
                        t.backend()
                            .buffer()
                            .content
                            .iter()
                            .any(|c| c.modifier.contains(Modifier::REVERSED))
                    );
                }
            }
        }
    }
}
#[test]
fn tree_tab_returns_focus_instead_of_switching_content() {
    let mut v = patch();
    v.reopen(Origin::Tree);
    v.update(&key(KeyCode::Tab));
    assert!(!v.active());
    assert!(!v.whole_file());
    assert_eq!(v.take_intents(), vec![Intent::Returned]);
}

#[test]
fn pages_overlap_wrapped_rows_and_new_selection_survives_old_copy() {
    let mut v = code(
        &(0..60)
            .map(|i| format!("line-{i:02}: {}", "x".repeat(60)))
            .collect::<Vec<_>>()
            .join("\n"),
    );
    let first = screen(&mut v, 40, 12, Presentation::default());
    assert!(first.contains("line-00"));
    v.update(&key(KeyCode::PageDown));
    let second = screen(&mut v, 40, 12, Presentation::default());
    assert_ne!(first, second);
    v.update(&key(KeyCode::PageUp));
    assert_eq!(screen(&mut v, 40, 12, Presentation::default()), first);
    let mut long = code(&"abcdef ".repeat(300));
    let first = screen(&mut long, 40, 12, Presentation::default());
    long.update(&key(KeyCode::PageDown));
    let second = screen(&mut long, 40, 12, Presentation::default());
    assert_ne!(first, second);
    long.update(&key(KeyCode::PageUp));
    assert_eq!(screen(&mut long, 40, 12, Presentation::default()), first);
    v.update(&key(KeyCode::Char('v')));
    v.update(&key(KeyCode::Char('y')));
    let r = request(&mut v);
    v.update(&key(KeyCode::Down));
    let selection = v.selection();
    v.complete(&r, Outcome::Copied)
        .expect("valid inspection fixture or result");
    assert_eq!(v.selection(), selection);
    assert!(selection.is_some());
}
#[test]
fn numbered_excerpts_unicode_find_queued_completion_and_invalid_replacement_are_safe() {
    let mut v = Inspector::new(
        Item::new("excerpt", "file.rs"),
        State::Ready(Content::CodeAt {
            text: "İstanbul 👩‍💻 e\u{301}\nistanbul".into(),
            first_line: 168,
        }),
    )
    .expect("valid inspection fixture or result");
    assert_eq!(v.line(), Some(168));
    v.update(&key(KeyCode::Char('/')));
    v.update(&Event::Paste("i".into()));
    assert_eq!(v.match_count(), 2);
    v.update(&key(KeyCode::Enter));
    assert_eq!(v.line(), Some(168));
    v.update(&key(KeyCode::Char('N')));
    assert_eq!(v.line(), Some(169));
    v.update(&key(KeyCode::Esc));
    v.update(&key(KeyCode::Char('v')));
    v.update(&key(KeyCode::Char('y')));
    let r = v
        .pending()
        .cloned()
        .expect("valid inspection fixture or result");
    v.complete(&r, Outcome::Copied)
        .expect("valid inspection fixture or result");
    assert_eq!(v.take_intents(), Vec::<Intent>::new());
    let mut item = v.item().clone();
    item.revision += 1;
    assert_eq!(
        v.replace(
            item,
            State::Ready(Content::Diff {
                patch: "bad".into(),
                new_file: None
            })
        ),
        Err(Invalid::Patch)
    );
    assert_eq!(v.line(), Some(169));
}
proptest::proptest! {
    #[test]
    fn arbitrary_navigation_keeps_source_position_and_selection_bounded(keys in proptest::collection::vec(0u8..10,0..80)) {
        let mut v=code("one\n東京\nthree\nfour\nfive");
        for n in keys {let code=[KeyCode::Up,KeyCode::Down,KeyCode::PageUp,KeyCode::PageDown,KeyCode::Home,KeyCode::End,KeyCode::Char('v'),KeyCode::Char('w'),KeyCode::Right,KeyCode::Left][usize::from(n)];v.update(&key(code));screen(&mut v,40,8,Presentation::default());proptest::prop_assert!(v.line().is_some_and(|line|(1..=5).contains(&line)));if let Some(r)=v.selection() {proptest::prop_assert!(r.start<r.end&&r.end<=5);}}
    }
}

#[test]
fn dense_unicode_matches_remain_bounded_and_refining_clears_the_limit() {
    let mut v = code(&("a".repeat(65_536) + "\nsecond"));
    v.update(&key(KeyCode::Char('/')));
    v.update(&Event::Paste("a".into()));
    assert_eq!(v.match_count(), 65_536);
    assert!(v.notice().contains("find limited"));
    let text = screen(&mut v, 80, 12, Presentation::default());
    assert!(text.contains("matches"));
    v.update(&key(KeyCode::Esc));
    v.update(&key(KeyCode::Char('/')));
    v.update(&Event::Paste("second".into()));
    assert_eq!(v.match_count(), 1);
    assert!(!v.notice().contains("find limited"));
}

#[test]
fn find_and_line_fields_show_their_text_and_claim_only_their_cursor() {
    let mut v = code("one\ntwo");
    v.update(&key(KeyCode::Char('/')));
    v.update(&Event::Paste("needle".into()));
    let mut terminal = Terminal::new(TestBackend::new(80, 12)).expect("test backend");
    terminal
        .draw(|f| v.render(f, f.area(), Presentation::default()))
        .expect("render");
    let text = super::components::dump(terminal.backend().buffer());
    assert!(text.contains("needle"), "{text}");
    assert!(!text.contains("draft"), "{text}");
    let position = terminal.get_cursor_position().expect("cursor");
    assert_eq!(position.y, 8);
    assert!(position.x >= 6);
    v.update(&key(KeyCode::Esc));
    v.update(&key(KeyCode::Char(':')));
    v.update(&Event::Paste("123".into()));
    assert!(screen(&mut v, 40, 12, Presentation::default()).contains("123"));
}
