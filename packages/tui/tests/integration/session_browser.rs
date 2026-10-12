//! Stable-ID browsing, isolated fields and exact discovery/action lifetimes.

use canonic_tui::{
    chat::{Anchor, Chat, Intent as ChatIntent, Role, Turn},
    event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers},
    picker::Choice,
    ratatui::{Terminal, backend::TestBackend},
    session_browser::{
        Discovery, Intent, Invalid, Kind, Offered, Outcome, Request, Session, SessionBrowser,
        Snapshot, Status,
    },
    theme::{ColorMode, Presentation, Symbols, Theme},
};
use std::time::Duration;
fn key(code: KeyCode) -> Event {
    Event::Key(KeyEvent::new(code, KeyModifiers::NONE))
}
fn press(b: &mut SessionBrowser, code: KeyCode) {
    b.update(&key(code));
}
fn snapshot() -> Snapshot {
    let mut rows = vec![];
    for (id, title) in [
        ("a", "Current conversation"),
        ("b", "Digest retries"),
        ("c", "Worker queue"),
    ] {
        let mut row = Session::new(id, title);
        row.revision = 1;
        row.group = "recent".into();
        row.project = "Acme".into();
        row.age = "now".into();
        row.actions = vec![
            Offered::new(Kind::Open, "open"),
            Offered::new(Kind::Rename, "rename"),
            Offered::new(Kind::Fork, "fork"),
            Offered::new(Kind::Move, "move"),
            Offered::new(Kind::Pin, "pin"),
            Offered::new(Kind::Delete, "delete"),
        ];
        row.actions[5].confirmation =
            format!("Delete '{title}'?\nAll 38 messages will be lost. This can't be undone.");
        row.destinations = vec![
            Choice::new("here", "Acme current"),
            Choice::new("next", "Acme destination"),
        ];
        row.destinations[0].disabled = Some("already here".into());
        rows.push(row);
    }
    rows[0].messages = "digest body match".into();
    let mut s = Snapshot::new(1, "all projects", rows);
    s.search_all = true;
    s
}
fn browser() -> SessionBrowser {
    SessionBrowser::new(snapshot(), "a").expect("valid session fixture")
}
fn action(b: &mut SessionBrowser) -> Request {
    b.take_intents()
        .into_iter()
        .find_map(|i| {
            if let Intent::Action(r) = i {
                Some(r)
            } else {
                None
            }
        })
        .expect("row request")
}
fn discovery(b: &mut SessionBrowser) -> Discovery {
    b.take_intents()
        .into_iter()
        .find_map(|i| {
            if let Intent::Discover(r) = i {
                Some(r)
            } else {
                None
            }
        })
        .expect("discovery request")
}
fn render(b: &mut SessionBrowser, w: u16, h: u16, p: Presentation) -> String {
    let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
    terminal.draw(|f| b.render_with(f, f.area(), p)).unwrap();
    let buffer = terminal.backend().buffer();
    (0..h)
        .map(|y| (0..w).map(|x| buffer[(x, y)].symbol()).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n")
}
#[test]
fn validates_whole_snapshots_and_keeps_old_tickets_on_invalid_replacements() {
    let mut b = browser();
    press(&mut b, KeyCode::Down);
    press(&mut b, KeyCode::Char('f'));
    let r = action(&mut b);
    let mut bad = snapshot();
    bad.revision = 2;
    bad.rows.push(bad.rows[0].clone());
    assert_eq!(b.replace(bad), Err(Invalid::Identity));
    assert_eq!(b.pending("b"), Some(&r));
    let mut cycle = snapshot();
    cycle.rows[0].parent = Some("b".into());
    cycle.rows[1].parent = Some("a".into());
    assert!(matches!(
        SessionBrowser::new(cycle, "a"),
        Err(Invalid::Parent)
    ));
    let mut huge = snapshot();
    huge.rows[0].preview = "x".repeat(65537);
    assert!(matches!(
        SessionBrowser::new(huge, "a"),
        Err(Invalid::Limit)
    ));
    let mut duplicate = snapshot();
    let repeated = duplicate.rows[0].actions[0].clone();
    duplicate.rows[0].actions.push(repeated);
    assert!(matches!(
        SessionBrowser::new(duplicate, "a"),
        Err(Invalid::Contract)
    ));
    let mut disabled = snapshot();
    disabled.rows[0].actions[5].confirmation.clear();
    assert!(matches!(
        SessionBrowser::new(disabled, "a"),
        Err(Invalid::Contract)
    ));
    assert!(SessionBrowser::new(snapshot(), "bad\nidentity").is_err());
}
#[test]
fn refresh_preserves_ids_and_repairs_next_previous_and_empty_selection() {
    let mut b = browser();
    press(&mut b, KeyCode::Down);
    let mut s = snapshot();
    s.revision = 2;
    s.rows.rotate_right(1);
    assert!(b.replace(s.clone()).unwrap());
    assert_eq!(b.selected(), Some("b"));
    s.revision = 3;
    s.rows.retain(|r| r.id != "b");
    b.replace(s.clone()).unwrap();
    assert_eq!(b.selected(), Some("a"));
    s.revision = 4;
    s.rows.retain(|r| r.id != "a");
    b.replace(s.clone()).unwrap();
    assert_eq!(b.selected(), Some("c"));
    s.revision = 5;
    s.rows.clear();
    b.replace(s).unwrap();
    assert_eq!(b.selected(), None);
    press(&mut b, KeyCode::Enter);
    assert_eq!(b.take_intents(), Vec::<Intent>::new());
}
#[test]
fn local_search_ranks_titles_before_messages_and_never_activates_from_paste() {
    let mut b = browser();
    press(&mut b, KeyCode::Char('/'));
    b.update(&Event::Paste("DIGEST".into()));
    assert_eq!(b.visible_ids(), vec![Some("b"), Some("a"), None]);
    assert_eq!(b.take_intents(), Vec::<Intent>::new());
    assert!(!b.hint(100).contains("r rename"));
    b.update(&Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::SHIFT,
    )));
    assert_eq!(b.query(), "DIGEST");
    assert_eq!(b.take_intents(), Vec::<Intent>::new());
    press(&mut b, KeyCode::Esc);
    assert_eq!(b.query(), "DIGEST");
    assert!(!b.closed());
    press(&mut b, KeyCode::Esc);
    assert_eq!(b.query(), "");
    assert!(!b.closed());
    press(&mut b, KeyCode::Esc);
    assert!(b.closed());
}
#[test]
fn remote_queries_reject_foreign_stale_duplicate_and_undrained_results() {
    let mut b = browser();
    press(&mut b, KeyCode::Char('/'));
    b.update(&Event::Paste("digest".into()));
    press(&mut b, KeyCode::End);
    press(&mut b, KeyCode::Enter);
    let old = discovery(&mut b);
    b.update(&Event::Paste(" new".into()));
    assert!(!b.complete_discovery(&old, Ok(snapshot())).unwrap());
    press(&mut b, KeyCode::End);
    press(&mut b, KeyCode::Enter);
    let current = discovery(&mut b);
    assert_ne!(old.ticket, current.ticket);
    let mut other = browser();
    press(&mut other, KeyCode::Char('/'));
    other.update(&Event::Paste("digest new".into()));
    press(&mut other, KeyCode::End);
    press(&mut other, KeyCode::Enter);
    let foreign = discovery(&mut other);
    let mut result = snapshot();
    result.revision = 2;
    result.rows[0].title = "Archived match".into();
    result.rows[0].revision += 1;
    assert!(!b.complete_discovery(&foreign, Ok(result.clone())).unwrap());
    assert!(b.complete_discovery(&current, Ok(result.clone())).unwrap());
    assert!(!b.complete_discovery(&current, Ok(result)).unwrap());
    b.update(&Event::Paste(" x".into()));
    press(&mut b, KeyCode::End);
    press(&mut b, KeyCode::Enter);
    b.update(&Event::Paste(" y".into()));
    assert_eq!(b.take_intents(), Vec::<Intent>::new());
}
#[test]
fn remote_failure_keeps_loaded_matches_and_explicit_search_retry_issues_fresh_ticket() {
    let mut b = browser();
    press(&mut b, KeyCode::Char('/'));
    b.update(&Event::Paste("digest".into()));
    press(&mut b, KeyCode::End);
    press(&mut b, KeyCode::Enter);
    let first = discovery(&mut b);
    assert!(b.complete_discovery(&first, Err("offline".into())).unwrap());
    assert_eq!(b.visible_ids(), vec![Some("b"), Some("a"), None]);
    assert!(render(&mut b, 80, 20, Presentation::default()).contains("offline"));
    press(&mut b, KeyCode::Enter);
    let retry = discovery(&mut b);
    assert_ne!(retry.ticket, first.ticket);
    press(&mut b, KeyCode::Esc);
    assert!(!b.complete_discovery(&retry, Ok(snapshot())).unwrap());
    assert!(!b.closed());
}
#[test]
fn paging_is_single_flight_and_failure_requires_explicit_retry_with_same_cursor() {
    let mut s = snapshot();
    s.more = Some("opaque page cursor".into());
    let mut b = SessionBrowser::new(s, "a").unwrap();
    press(&mut b, KeyCode::End);
    let first = discovery(&mut b);
    press(&mut b, KeyCode::End);
    assert_eq!(b.take_intents(), Vec::<Intent>::new());
    assert_eq!(b.selected(), Some("c"));
    b.complete_discovery(&first, Err("locked".into())).unwrap();
    press(&mut b, KeyCode::End);
    assert_eq!(b.take_intents(), Vec::<Intent>::new());
    press(&mut b, KeyCode::Char('r'));
    let retry = discovery(&mut b);
    assert_eq!(retry.cursor, first.cursor);
    assert_ne!(retry.ticket, first.ticket);
    let mut s = snapshot();
    s.revision = 2;
    s.rows.insert(0, Session::new("new", "New first row"));
    b.complete_discovery(&retry, Ok(s)).unwrap();
    assert_eq!(b.selected(), Some("c"));
}
#[test]
fn row_requests_are_single_use_and_atomic_for_wrong_outcome_or_invalid_snapshot() {
    let mut b = browser();
    press(&mut b, KeyCode::Down);
    press(&mut b, KeyCode::Char('p'));
    let r = action(&mut b);
    press(&mut b, KeyCode::Char('p'));
    press(&mut b, KeyCode::Enter);
    assert_eq!(b.take_intents(), Vec::<Intent>::new());
    assert!(!b.complete(&r, Outcome::Opened).unwrap());
    let mut s = snapshot();
    s.revision = 2;
    s.rows[1].revision = 2;
    s.rows[1].pinned = true;
    let mut bad = s.clone();
    bad.rows.push(bad.rows[1].clone());
    assert_eq!(
        b.complete(&r, Outcome::Applied(bad)),
        Err(Invalid::Identity)
    );
    assert_eq!(b.pending("b"), Some(&r));
    let mut other = browser();
    press(&mut other, KeyCode::Down);
    press(&mut other, KeyCode::Char('p'));
    let foreign = action(&mut other);
    assert!(!b.complete(&foreign, Outcome::Applied(s.clone())).unwrap());
    assert!(b.complete(&r, Outcome::Applied(s.clone())).unwrap());
    assert!(!b.complete(&r, Outcome::Applied(s)).unwrap());
    assert_eq!(b.selected(), Some("b"));
}
#[test]
fn concurrent_rows_survive_unrelated_refresh_and_material_change_prunes_only_its_intent() {
    let mut b = browser();
    press(&mut b, KeyCode::Down);
    press(&mut b, KeyCode::Char('f'));
    press(&mut b, KeyCode::Down);
    press(&mut b, KeyCode::Char('p'));
    let one = b.pending("b").unwrap().clone();
    let two = b.pending("c").unwrap().clone();
    let mut s = snapshot();
    s.revision = 2;
    s.rows[1].revision += 1;
    b.replace(s).unwrap();
    assert!(b.pending("b").is_none());
    assert_eq!(b.pending("c"), Some(&two));
    let intents = b.take_intents();
    assert_eq!(intents, vec![Intent::Action(two)]);
    assert!(!b.complete(&one, Outcome::Failed("late".into())).unwrap());
}
#[test]
fn rename_selects_old_title_preserves_over_limit_and_refuses_whole_resource_paste() {
    let mut s = snapshot();
    s.rows[1].rename_limit = 4;
    let mut b = SessionBrowser::new(s, "a").unwrap();
    press(&mut b, KeyCode::Down);
    press(&mut b, KeyCode::Char('r'));
    b.update(&Event::Paste("東京abc".into()));
    assert_eq!(b.rename_text(), Some("東京abc"));
    press(&mut b, KeyCode::Enter);
    assert_eq!(b.take_intents(), Vec::<Intent>::new());
    b.update(&Event::Paste("x".repeat(4097)));
    assert_eq!(b.rename_text(), Some("東京abc"));
    b.update(&Event::Paste("new\nline".into()));
    assert_eq!(b.rename_text(), Some("東京abc"));
    b.update(&Event::Key(KeyEvent::new(
        KeyCode::Char('j'),
        KeyModifiers::CONTROL,
    )));
    assert_eq!(b.rename_text(), Some("東京abc"));
    press(&mut b, KeyCode::Esc);
    assert_eq!(b.rename_text(), None);
    assert_eq!(b.snapshot().rows[1].title, "Digest retries");
}
#[test]
fn refused_rename_retains_draft_and_retry_requires_save_with_new_ticket() {
    let mut b = browser();
    press(&mut b, KeyCode::Down);
    press(&mut b, KeyCode::Char('r'));
    b.update(&Event::Paste("Worker 409 retries".into()));
    press(&mut b, KeyCode::Enter);
    let first = action(&mut b);
    assert!(render(&mut b, 80, 20, Presentation::default()).contains("Worker 409 retries"));
    b.complete(&first, Outcome::Failed("title taken".into()))
        .unwrap();
    assert_eq!(b.rename_text(), Some("Worker 409 retries"));
    press(&mut b, KeyCode::Enter);
    let retry = action(&mut b);
    assert_ne!(first.ticket, retry.ticket);
    assert_eq!(retry.value, first.value);
    let mut s = snapshot();
    s.revision = 2;
    s.rows[1].revision += 1;
    s.rows[1].title = "Host accepted name".into();
    assert!(b.complete(&retry, Outcome::Applied(s)).unwrap());
    assert_eq!(b.snapshot().rows[1].title, "Host accepted name");
}
#[test]
fn rename_failure_on_another_row_does_not_steal_focus_and_restores_draft_on_return() {
    let mut b = browser();
    press(&mut b, KeyCode::Down);
    press(&mut b, KeyCode::Char('r'));
    b.update(&Event::Paste("my title".into()));
    press(&mut b, KeyCode::Enter);
    let r = action(&mut b);
    press(&mut b, KeyCode::Down);
    b.complete(&r, Outcome::Failed("not renamed".into()))
        .unwrap();
    assert_eq!(b.selected(), Some("c"));
    assert_eq!(b.rename_text(), None);
    press(&mut b, KeyCode::Up);
    press(&mut b, KeyCode::Char('r'));
    assert_eq!(b.rename_text(), Some("my title"));
}
#[test]
fn action_availability_and_move_destinations_are_host_owned() {
    let mut s = snapshot();
    s.rows[1].actions[2].disabled = Some("still working".into());
    s.rows[1].actions.retain(|a| a.kind != Kind::Pin);
    let mut b = SessionBrowser::new(s, "a").unwrap();
    press(&mut b, KeyCode::Down);
    press(&mut b, KeyCode::Char('f'));
    press(&mut b, KeyCode::Char('p'));
    assert_eq!(b.take_intents(), Vec::<Intent>::new());
    press(&mut b, KeyCode::Char('a'));
    assert!(render(&mut b, 80, 22, Presentation::default()).contains("still working"));
    b.update(&Event::Paste("delete".into()));
    assert_eq!(b.take_intents(), Vec::<Intent>::new());
    press(&mut b, KeyCode::Esc);
    press(&mut b, KeyCode::Char('m'));
    press(&mut b, KeyCode::Enter);
    let r = action(&mut b);
    assert_eq!(r.kind, Kind::Move);
    assert_eq!(r.value, "next");
}
#[test]
fn delete_safe_default_numeric_highlight_repeats_paste_clock_and_material_changes() {
    let mut b = browser();
    press(&mut b, KeyCode::Down);
    press(&mut b, KeyCode::Char('d'));
    press(&mut b, KeyCode::Enter);
    assert!(!b.confirming());
    assert_eq!(b.take_intents(), Vec::<Intent>::new());
    press(&mut b, KeyCode::Char('d'));
    press(&mut b, KeyCode::Char('2'));
    assert_eq!(b.take_intents(), Vec::<Intent>::new());
    press(&mut b, KeyCode::Enter);
    assert_eq!(b.take_intents(), Vec::<Intent>::new());
    b.update(&Event::Key(KeyEvent::new_with_kind(
        KeyCode::Enter,
        KeyModifiers::NONE,
        KeyEventKind::Repeat,
    )));
    assert_eq!(b.take_intents(), Vec::<Intent>::new());
    b.update(&Event::Paste("yes".into()));
    press(&mut b, KeyCode::Enter);
    assert_eq!(b.take_intents(), Vec::<Intent>::new());
    b.tick(Duration::from_secs(4));
    press(&mut b, KeyCode::Enter);
    assert_eq!(b.take_intents(), Vec::<Intent>::new());
    press(&mut b, KeyCode::Enter);
    let r = action(&mut b);
    assert_eq!(r.kind, Kind::Delete);
    assert_eq!(b.snapshot().rows.len(), 3);
    b.complete(&r, Outcome::Failed("still writing".into()))
        .unwrap();
    assert_eq!(b.snapshot().rows.len(), 3);
    press(&mut b, KeyCode::Char('d'));
    press(&mut b, KeyCode::Char('2'));
    press(&mut b, KeyCode::Enter);
    let mut s = snapshot();
    s.revision = 2;
    s.rows[1].revision += 1;
    b.replace(s).unwrap();
    assert!(!b.confirming());
    press(&mut b, KeyCode::Enter);
    assert_eq!(action(&mut b).kind, Kind::Open);
}
#[test]
fn deletion_requires_actual_removal_and_last_row_has_no_stale_selection() {
    let mut s = snapshot();
    s.rows.truncate(1);
    let mut b = SessionBrowser::new(s, "other").unwrap();
    press(&mut b, KeyCode::Char('d'));
    press(&mut b, KeyCode::Char('2'));
    press(&mut b, KeyCode::Enter);
    press(&mut b, KeyCode::Enter);
    let r = action(&mut b);
    let mut unchanged = b.snapshot().clone();
    unchanged.revision += 1;
    assert!(!b.complete(&r, Outcome::Applied(unchanged.clone())).unwrap());
    unchanged.rows.clear();
    assert!(b.complete(&r, Outcome::Applied(unchanged)).unwrap());
    assert_eq!(b.selected(), None);
    assert!(render(&mut b, 40, 12, Presentation::default()).contains("No conversations"));
}
#[test]
fn fork_and_pin_use_supplied_snapshots_and_follow_actual_ids_without_sorting() {
    let mut b = browser();
    press(&mut b, KeyCode::Down);
    press(&mut b, KeyCode::Char('f'));
    let r = action(&mut b);
    let mut s = snapshot();
    s.revision = 2;
    let mut fork = s.rows[1].clone();
    fork.id = "actual-fork".into();
    fork.parent = Some("b".into());
    s.rows.insert(2, fork);
    assert!(
        b.complete(
            &r,
            Outcome::Forked {
                snapshot: s.clone(),
                id: "actual-fork".into()
            }
        )
        .unwrap()
    );
    assert_eq!(b.selected(), Some("actual-fork"));
    assert_eq!(
        b.visible_ids(),
        vec![Some("a"), Some("b"), Some("actual-fork"), Some("c")]
    );
    press(&mut b, KeyCode::Char('p'));
    let r = action(&mut b);
    s.revision = 3;
    s.rows[2].revision += 1;
    s.rows[2].pinned = true;
    s.rows.rotate_right(2);
    b.complete(&r, Outcome::Applied(s.clone())).unwrap();
    assert_eq!(b.selected(), Some("actual-fork"));
    assert_eq!(b.snapshot().rows[0].id, s.rows[0].id);
}
#[test]
fn close_and_reopen_invalidate_actions_discovery_and_undrained_intents() {
    let mut b = browser();
    press(&mut b, KeyCode::Down);
    press(&mut b, KeyCode::Enter);
    let r = action(&mut b);
    press(&mut b, KeyCode::Esc);
    assert_eq!(b.take_intents(), vec![Intent::Returned("a".into())]);
    assert!(!b.complete(&r, Outcome::Opened).unwrap());
    b.reopen("a").unwrap();
    press(&mut b, KeyCode::Down);
    press(&mut b, KeyCode::Enter);
    let fresh = action(&mut b);
    assert_ne!(fresh.ticket, r.ticket);
    assert!(b.complete(&fresh, Outcome::Opened).unwrap());
    assert_eq!(b.take_intents(), vec![Intent::Opened("b".into())]);
}
#[test]
fn chat_entry_is_opt_in_and_keeps_document_selection_undo_mode_anchor_and_unread() {
    let mut chat = Chat::new();
    assert!(!chat.request_sessions());
    chat.set_sessions_available(true);
    chat.set_input("and then open the PR");
    for i in 0..20 {
        chat.push_record(Turn::new(
            &format!("turn-{i}"),
            Role::Assistant,
            "acme",
            "Read this conversation before switching.",
        ));
    }
    chat.jump(Anchor {
        message: "turn-2".into(),
        part: "body".into(),
        offset: 0,
    });
    chat.composer_mut().update(&key(KeyCode::Left));
    chat.composer_mut().update(&Event::Key(KeyEvent::new(
        KeyCode::Left,
        KeyModifiers::SHIFT,
    )));
    let document = chat.composer().editor().document();
    let caret = chat.composer().editor().cursor();
    let selection = chat.composer().editor().selection();
    let anchor = chat.viewport_anchor().cloned();
    let mode = chat.composer().mode().clone();
    chat.update(&Event::Key(KeyEvent::new(
        KeyCode::Char('x'),
        KeyModifiers::CONTROL,
    )));
    chat.update(&key(KeyCode::Char('o')));
    assert_eq!(
        chat.take_intents(),
        vec![ChatIntent::Sessions(chat.conversation())]
    );
    let mut b = browser();
    press(&mut b, KeyCode::Char('/'));
    b.update(&Event::Paste("independent query".into()));
    chat.push_record(Turn::new(
        "new",
        Role::Assistant,
        "acme",
        "Unread output while browsing.",
    ));
    press(&mut b, KeyCode::Esc);
    press(&mut b, KeyCode::Esc);
    press(&mut b, KeyCode::Esc);
    assert_eq!(chat.composer().editor().document(), document);
    assert_eq!(chat.composer().editor().cursor(), caret);
    assert_eq!(chat.composer().editor().selection(), selection);
    assert_eq!(chat.composer().mode(), &mode);
    assert_eq!(chat.viewport_anchor(), anchor.as_ref());
    assert!(chat.unread_count() > 0);
}
#[test]
fn profiles_tiny_navigation_and_full_delete_wording_remain_safe_and_reachable() {
    for colors in [
        ColorMode::Ansi16,
        ColorMode::Ansi256,
        ColorMode::TrueColor,
        ColorMode::NoColor,
    ] {
        for symbols in [Symbols::Ascii, Symbols::Unicode] {
            for theme in [Theme::Dark, Theme::Light] {
                let mut b = browser();
                for (w, h) in [(1, 1), (12, 4), (40, 12), (64, 20), (100, 24)] {
                    render(
                        &mut b,
                        w,
                        h,
                        Presentation {
                            colors,
                            symbols,
                            theme,
                        },
                    );
                    press(&mut b, KeyCode::Down);
                    assert!(b.selected().is_some());
                }
            }
        }
    }
    let mut s = snapshot();
    s.rows[1].actions[5].confirmation = format!(
        "{}FINAL LOSS WORDING",
        "Every supplied message is lost. ".repeat(80)
    );
    let mut b = SessionBrowser::new(s, "a").unwrap();
    press(&mut b, KeyCode::Down);
    press(&mut b, KeyCode::Char('d'));
    let mut found = false;
    for _ in 0..100 {
        let text = render(&mut b, 40, 12, Presentation::default());
        if text.contains("FINAL LOSS WORDING") {
            found = true;
            break;
        }
        press(&mut b, KeyCode::PageDown);
    }
    assert!(found);
    assert_eq!(b.take_intents(), Vec::<Intent>::new());
    let mut s = snapshot();
    s.rows[2].parent = Some("b".into());
    let mut b = SessionBrowser::new(s, "a").unwrap();
    press(&mut b, KeyCode::End);
    let ascii = render(
        &mut b,
        40,
        12,
        Presentation {
            symbols: Symbols::Ascii,
            ..Default::default()
        },
    );
    assert!(ascii.contains(">`-"));
    assert!(render(&mut b, 12, 4, Presentation::default()).contains("sessions"));
    let mut b = browser();
    assert!(b.hint(100).contains("d delete"));
    assert!(b.hint(40).contains("a actions"));
    b.update(&key(KeyCode::Char('/')));
    let hint = b.hint(100);
    assert!(hint.contains("esc leave search"));
    assert!(!hint.contains("d delete"));
}

#[test]
fn initial_load_error_retries_without_losing_a_query_typed_during_loading() {
    let mut s = snapshot();
    s.rows.clear();
    s.status = Status::Failed("host index is locked".into());
    let mut b = SessionBrowser::new(s, "a").unwrap();
    press(&mut b, KeyCode::Char('r'));
    let r = discovery(&mut b);
    press(&mut b, KeyCode::Char('/'));
    b.update(&Event::Paste("digest".into()));
    assert!(!render(&mut b, 80, 20, Presentation::default()).contains("No conversations"));
    let mut result = snapshot();
    result.revision = 2;
    assert!(b.complete_discovery(&r, Ok(result)).unwrap());
    assert_eq!(b.query(), "digest");
    assert_eq!(b.visible_ids(), vec![Some("b"), Some("a"), None]);
}
#[test]
fn material_changes_must_advance_row_revision_and_failed_validation_is_atomic() {
    let mut b = browser();
    press(&mut b, KeyCode::Down);
    press(&mut b, KeyCode::Char('p'));
    let r = action(&mut b);
    let mut changed = snapshot();
    changed.revision = 2;
    changed.rows[1].actions[5].confirmation = "Different deletion loss".into();
    assert_eq!(b.replace(changed), Err(Invalid::Contract));
    assert_eq!(b.pending("b"), Some(&r));
    let mut changed = snapshot();
    changed.revision = 2;
    changed.rows[1].title = "Changed title".into();
    assert_eq!(
        b.complete(&r, Outcome::Applied(changed)),
        Err(Invalid::Contract)
    );
    assert_eq!(b.pending("b"), Some(&r));
    let mut changed = snapshot();
    changed.revision = 2;
    changed.rows[1].age = "1m".into();
    changed.rows[1].preview = "new output".into();
    b.replace(changed).unwrap();
    assert_eq!(b.pending("b"), Some(&r));
}
#[test]
fn remote_search_hiding_loaded_pending_rows_does_not_cancel_their_host_requests() {
    let mut b = browser();
    press(&mut b, KeyCode::Down);
    press(&mut b, KeyCode::Char('f'));
    let action_request = action(&mut b);
    press(&mut b, KeyCode::Char('/'));
    b.update(&Event::Paste("queue".into()));
    press(&mut b, KeyCode::End);
    press(&mut b, KeyCode::Enter);
    let search = discovery(&mut b);
    let mut result = snapshot();
    result.revision = 2;
    result.rows = vec![result.rows[2].clone()];
    assert!(b.complete_discovery(&search, Ok(result)).unwrap());
    assert_eq!(b.visible_ids(), vec![Some("c")]);
    assert_eq!(b.pending("b"), Some(&action_request));
    assert!(
        b.complete(
            &action_request,
            Outcome::Failed("cannot fork while busy".into())
        )
        .unwrap()
    );
    b.update(&Event::Key(KeyEvent::new(
        KeyCode::Char('a'),
        KeyModifiers::CONTROL,
    )));
    b.update(&Event::Key(KeyEvent::new(
        KeyCode::End,
        KeyModifiers::SHIFT,
    )));
    b.update(&key(KeyCode::Backspace));
    assert_eq!(b.snapshot().revision, 2);
}
#[test]
fn remote_only_pending_action_survives_query_changes_and_open_is_single_flight() {
    let mut b = browser();
    press(&mut b, KeyCode::Char('/'));
    b.update(&Event::Paste("remote".into()));
    press(&mut b, KeyCode::End);
    press(&mut b, KeyCode::Enter);
    let search = discovery(&mut b);
    let mut result = snapshot();
    result.revision = 2;
    let mut row = result.rows[1].clone();
    row.id = "remote".into();
    row.title = "Remote session".into();
    result.rows = vec![row];
    b.complete_discovery(&search, Ok(result)).unwrap();
    press(&mut b, KeyCode::Esc);
    press(&mut b, KeyCode::Char('f'));
    let r = action(&mut b);
    press(&mut b, KeyCode::Char('/'));
    b.update(&Event::Paste(" new".into()));
    assert_eq!(b.pending("remote"), Some(&r));
    assert!(b.complete(&r, Outcome::Failed("offline".into())).unwrap());
    press(&mut b, KeyCode::Esc);
    press(&mut b, KeyCode::Esc);
    press(&mut b, KeyCode::Down);
    press(&mut b, KeyCode::Enter);
    let first = action(&mut b);
    press(&mut b, KeyCode::Down);
    press(&mut b, KeyCode::Enter);
    assert_eq!(b.take_intents(), Vec::<Intent>::new());
    assert_eq!(b.pending(&first.id), Some(&first));
}
#[test]
fn pending_request_budget_is_explicit_without_mutating_the_next_row() {
    let mut s = snapshot();
    s.rows = (0..257)
        .map(|i| {
            let mut r = s.rows[1].clone();
            r.id = format!("session-{i}");
            r
        })
        .collect();
    let mut b = SessionBrowser::new(s, "origin").unwrap();
    for _ in 0..256 {
        press(&mut b, KeyCode::Char('p'));
        press(&mut b, KeyCode::Down);
    }
    assert_eq!(b.take_intents().len(), 256);
    press(&mut b, KeyCode::Char('p'));
    assert!(b.pending("session-256").is_none());
    assert!(
        render(&mut b, 100, 24, Presentation::default())
            .contains("256 requests are already pending")
    );
}
#[test]
fn per_session_queue_draft_and_editor_history_survive_host_owned_switching() {
    let mut first = Chat::new();
    first.set_sessions_available(true);
    first.composer_mut().set_working(true);
    first.set_input("queued for first");
    first.composer_mut().update(&key(KeyCode::Enter));
    first.composer_mut().set_working(false);
    first.set_input("first draft");
    first.update(&key(KeyCode::Char('!')));
    let mut second = Chat::new();
    second.set_input("second draft");
    let first_queue = first.composer().queue()[0].text.clone();
    let second_doc = second.composer().editor().document();
    first.request_sessions();
    first.take_intents();
    let mut b = browser();
    press(&mut b, KeyCode::Down);
    press(&mut b, KeyCode::Enter);
    let r = action(&mut b);
    b.complete(&r, Outcome::Opened).unwrap();
    assert_eq!(b.take_intents(), vec![Intent::Opened("b".into())]);
    assert_eq!(second.composer().editor().document(), second_doc);
    b.reopen("b").unwrap();
    press(&mut b, KeyCode::Home);
    press(&mut b, KeyCode::Enter);
    let r = action(&mut b);
    b.complete(&r, Outcome::Opened).unwrap();
    assert_eq!(first.composer().queue()[0].text, first_queue);
    assert_eq!(first.input(), "first draft!");
    first.update(&Event::Key(KeyEvent::new(
        KeyCode::Char('z'),
        KeyModifiers::CONTROL,
    )));
    assert_eq!(first.input(), "first draft");
    assert_eq!(second.input(), "second draft");
}

#[test]
fn remote_result_paging_preserves_selection_and_changed_queries_invalidate_page_tickets() {
    let mut b = browser();
    press(&mut b, KeyCode::Char('/'));
    b.update(&Event::Paste("digest".into()));
    press(&mut b, KeyCode::End);
    press(&mut b, KeyCode::Enter);
    let search = discovery(&mut b);
    let mut result = snapshot();
    result.revision = 2;
    result.rows = vec![result.rows[1].clone()];
    result.more = Some("remote-page".into());
    b.complete_discovery(&search, Ok(result.clone())).unwrap();
    b.update(&Event::Key(KeyEvent::new_with_kind(
        KeyCode::End,
        KeyModifiers::NONE,
        KeyEventKind::Repeat,
    )));
    let page = discovery(&mut b);
    assert!(page.search);
    assert_eq!(page.query, "digest");
    assert_eq!(page.cursor, "remote-page");
    result.revision = 3;
    let mut extra = result.rows[0].clone();
    extra.id = "remote-extra".into();
    result.rows.push(extra);
    b.complete_discovery(&page, Ok(result.clone())).unwrap();
    assert_eq!(b.selected(), Some("b"));
    press(&mut b, KeyCode::End);
    let next = discovery(&mut b);
    b.update(&Event::Paste(" changed".into()));
    assert!(!b.complete_discovery(&next, Ok(result)).unwrap());
    assert_eq!(b.snapshot().revision, 3);
}
