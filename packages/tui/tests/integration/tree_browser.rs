//! Public tree lifecycles, topology, host races, focus and rendering boundaries.

use canonic_tui::{
    event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers},
    ratatui::{Terminal, backend::TestBackend},
    theme::{ColorMode, Presentation, Symbols, Theme},
    tree_browser::{
        Action, Children, Cursor, Intent, Kind, Node, Operation, Outcome, Placement, Request,
        Snapshot, TreeBrowser,
    },
};
fn key(code: KeyCode) -> Event {
    Event::Key(KeyEvent::new(code, KeyModifiers::NONE))
}
fn forest() -> Snapshot {
    let mut root = Node::new("root", "src", None);
    root.has_children = true;
    root.kind = Kind::Directory;
    let mut branch = Node::new("branch", "digest", Some("root"));
    branch.has_children = true;
    branch.kind = Kind::Directory;
    let mut lazy = Node::new("lazy", "profile", Some("root"));
    lazy.has_children = true;
    lazy.kind = Kind::Directory;
    lazy.children = Children::Unloaded;
    let mut file = Node::new("file", "schedule.ts", Some("branch"));
    let mut open = Action::new("open", "open diff");
    open.opens = true;
    open.pending = "opening".into();
    file.primary = Some("open".into());
    file.actions.push(open);
    let mut leaf = Node::new("leaf", "settings.ts", Some("branch"));
    leaf.actions.clone_from(&file.actions);
    leaf.primary.clone_from(&file.primary);
    Snapshot {
        revision: 1,
        title: "files".into(),
        nodes: vec![root, branch, file, leaf, lazy],
        ..Snapshot::default()
    }
}
fn tree() -> TreeBrowser {
    TreeBrowser::new(forest(), Placement::FullScreen).expect("valid forest fixture")
}
fn expanded() -> TreeBrowser {
    let mut t = tree();
    t.update(&key(KeyCode::Right));
    t.update(&key(KeyCode::Right));
    t.update(&key(KeyCode::Right));
    t
}
fn node(t: &TreeBrowser) -> Option<&str> {
    match t.cursor() {
        Some(Cursor::Node(id)) => Some(id),
        _ => None,
    }
}
fn request(t: &mut TreeBrowser) -> Request {
    t.take_intents()
        .into_iter()
        .find_map(|i| {
            if let Intent::Request(r) = i {
                Some(r)
            } else {
                None
            }
        })
        .expect("one identified host request")
}
fn draw(t: &mut TreeBrowser, w: u16, h: u16, p: Presentation) -> String {
    let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
    terminal.draw(|f| t.render(f, f.area(), p)).unwrap();
    super::components::dump(terminal.backend().buffer())
}
#[test]
fn arrows_close_then_climb_and_toggle_are_one_step() {
    let mut t = tree();
    assert_eq!(node(&t), Some("root"));
    t.update(&key(KeyCode::Right));
    assert!(t.expanded("root"));
    assert_eq!(node(&t), Some("root"));
    t.update(&key(KeyCode::Right));
    assert_eq!(node(&t), Some("branch"));
    t.update(&key(KeyCode::Right));
    t.update(&key(KeyCode::Right));
    assert_eq!(node(&t), Some("file"));
    t.update(&key(KeyCode::Left));
    assert_eq!(node(&t), Some("branch"));
    t.update(&key(KeyCode::Left));
    assert!(!t.expanded("branch"));
    assert_eq!(node(&t), Some("branch"));
    t.update(&key(KeyCode::Left));
    assert_eq!(node(&t), Some("root"));
    t.update(&key(KeyCode::Char(' ')));
    assert!(!t.expanded("root"));
}
#[test]
fn filters_keep_ancestors_without_changing_folds_or_loading() {
    let mut t = tree();
    t.update(&key(KeyCode::Char('/')));
    t.update(&Event::Paste("SCHEDULE".into()));
    let s = draw(&mut t, 64, 14, Presentation::default());
    assert!(s.contains("digest/") && s.contains("schedule.ts"));
    assert!(!s.contains("profile/") && !t.expanded("root"));
    assert_eq!(t.take_intents(), Vec::<Intent>::new());
    t.select(Cursor::Node("file".into()));
    t.update(&key(KeyCode::Enter));
    assert!(matches!(request(&mut t).operation, Operation::Action(_)));
    t.update(&key(KeyCode::Esc));
    assert_eq!(node(&t), Some("root"));
    assert_eq!(t.query(), "");
}
#[test]
fn filtered_paste_and_control_newlines_never_execute_or_split_query() {
    let mut t = expanded();
    t.update(&key(KeyCode::Char('/')));
    t.update(&Event::Paste("a*xr".into()));
    assert_eq!(t.query(), "a*xr");
    assert_eq!(t.take_intents(), Vec::<Intent>::new());
    t.update(&Event::Paste("bad\nquery".into()));
    t.update(&Event::Key(KeyEvent::new(
        KeyCode::Char('j'),
        KeyModifiers::CONTROL,
    )));
    assert_eq!(t.query(), "a*xr");
    t.update(&Event::Paste("a".repeat(4097)));
    assert_eq!(t.query(), "a*xr");
}
#[test]
fn refresh_preserves_cursor_folds_and_removal_climbs_old_ancestry() {
    let mut t = expanded();
    t.select(Cursor::Node("file".into()));
    let mut s = t.snapshot().clone();
    s.revision += 1;
    s.nodes
        .insert(0, Node::new("first", "new.ts", Some("branch")));
    t.replace(s).unwrap();
    assert_eq!(node(&t), Some("file"));
    assert!(t.expanded("branch"));
    let mut s = t.snapshot().clone();
    s.revision += 1;
    s.nodes.retain(|n| n.id != "file");
    t.replace(s).unwrap();
    assert_eq!(node(&t), Some("branch"));
    let mut s = t.snapshot().clone();
    s.revision += 1;
    s.nodes
        .retain(|n| n.id != "branch" && n.parent.as_deref() != Some("branch"));
    t.replace(s).unwrap();
    assert_eq!(node(&t), Some("root"));
}
#[test]
fn material_and_limits_are_atomic_and_keep_valid_pending_requests() {
    let mut t = expanded();
    t.select(Cursor::Node("file".into()));
    t.update(&key(KeyCode::Enter));
    let r = request(&mut t);
    let original = t.snapshot().clone();
    let mut s = original.clone();
    s.revision += 1;
    s.nodes[2].name = "changed".into();
    assert!(t.replace(s).is_err());
    let mut s = original.clone();
    s.revision += 1;
    s.nodes[2].name = "x".repeat(4097);
    assert!(t.replace(s).is_err());
    assert_eq!(t.snapshot(), &original);
    assert!(t.resolve(&r, Outcome::Opened));
}
#[test]
fn repeats_cycles_orphans_and_children_under_leaves_are_skipped() {
    let mut s = forest();
    s.nodes.push(s.nodes[2].clone());
    let mut loop_node = Node::new("loop", "loop", Some("loop"));
    loop_node.has_children = true;
    s.nodes.push(loop_node);
    s.nodes
        .push(Node::new("orphan", "missing", Some("missing")));
    s.nodes.push(Node::new("badchild", "bad", Some("file")));
    s.nodes.reverse();
    let t = TreeBrowser::new(s, Placement::FullScreen).unwrap();
    assert_eq!(t.skipped(), 4);
    assert_eq!(t.snapshot().nodes.len(), 5);
}
#[test]
fn lazy_children_are_single_flight_and_late_results_do_not_reopen() {
    let mut t = expanded();
    t.select(Cursor::Node("lazy".into()));
    t.update(&key(KeyCode::Right));
    let r = request(&mut t);
    t.update(&key(KeyCode::Right));
    assert_eq!(t.take_intents(), Vec::<Intent>::new());
    t.update(&key(KeyCode::Left));
    assert!(!t.expanded("lazy"));
    let mut s = t.snapshot().clone();
    s.revision += 1;
    s.nodes
        .iter_mut()
        .find(|n| n.id == "lazy")
        .unwrap()
        .children = Children::Ready;
    s.nodes.push(Node::new("api", "api.ts", Some("lazy")));
    assert!(t.resolve(&r, Outcome::Loaded(s)));
    assert_eq!(node(&t), Some("lazy"));
    assert!(!t.expanded("lazy"));
    t.update(&key(KeyCode::Right));
    assert_eq!(t.take_intents(), Vec::<Intent>::new());
    t.update(&key(KeyCode::Right));
    assert_eq!(node(&t), Some("api"));
}
#[test]
fn failed_child_load_is_selectable_and_retries_only_its_folder() {
    let mut t = expanded();
    t.select(Cursor::Node("lazy".into()));
    t.update(&key(KeyCode::Right));
    let r = request(&mut t);
    assert!(t.resolve(&r, Outcome::Failed("permission denied".into())));
    t.update(&key(KeyCode::Right));
    assert_eq!(t.cursor(), Some(&Cursor::Children(Some("lazy".into()))));
    t.update(&key(KeyCode::Enter));
    let retry = request(&mut t);
    assert_eq!(retry.node.as_deref(), Some("lazy"));
    assert_ne!(retry.ticket, r.ticket);
    assert!(!t.resolve(&r, Outcome::Failed("old".into())));
    assert!(draw(&mut t, 64, 16, Presentation::default()).contains("attempt 2"));
}
#[test]
fn pages_keep_auxiliary_cursor_and_pass_exact_cursor_to_host() {
    let mut s = forest();
    s.nodes[1].more = Some("opaque-page-2".into());
    let mut t = TreeBrowser::new(s, Placement::FullScreen).unwrap();
    t.update(&key(KeyCode::Char('*')));
    t.take_intents();
    assert!(t.select(Cursor::Children(Some("branch".into()))));
    t.update(&key(KeyCode::Enter));
    let r = request(&mut t);
    assert_eq!(
        r.operation,
        Operation::Load {
            cursor: Some("opaque-page-2".into())
        }
    );
    assert!(matches!(t.cursor(), Some(Cursor::Children(_))));
    let mut s = t.snapshot().clone();
    s.revision += 1;
    s.nodes[1].more = Some("opaque-page-3".into());
    s.nodes.push(Node::new("new", "next.ts", Some("branch")));
    assert!(t.resolve(&r, Outcome::Loaded(s)));
    assert_eq!(t.cursor(), Some(&Cursor::Children(Some("branch".into()))));
    t.update(&key(KeyCode::Enter));
    assert_eq!(
        request(&mut t).operation,
        Operation::Load {
            cursor: Some("opaque-page-3".into())
        }
    );
}
#[test]
fn root_error_empty_and_more_have_explicit_discovery() {
    let mut s = Snapshot {
        revision: 1,
        title: "files".into(),
        roots: Children::Failed("list failed".into()),
        ..Snapshot::default()
    };
    let mut t = TreeBrowser::new(s.clone(), Placement::FullScreen).unwrap();
    t.update(&key(KeyCode::Enter));
    let r = request(&mut t);
    assert!(r.node.is_none());
    s.revision = 2;
    s.roots = Children::Ready;
    assert!(t.resolve(&r, Outcome::Loaded(s)));
    assert!(t.cursor().is_none());
    assert!(draw(&mut t, 64, 12, Presentation::default()).contains("no children"));
}
#[test]
fn expansion_cascades_load_new_branches_but_closing_cancels_the_cascade() {
    let mut t = tree();
    t.update(&key(KeyCode::Char('*')));
    let r = request(&mut t);
    assert_eq!(r.node.as_deref(), Some("lazy"));
    let mut s = t.snapshot().clone();
    s.revision += 1;
    s.nodes[4].children = Children::Ready;
    let mut n = Node::new("next", "nested", Some("lazy"));
    n.has_children = true;
    n.children = Children::Unloaded;
    s.nodes.push(n);
    assert!(t.resolve(&r, Outcome::Loaded(s)));
    let next = request(&mut t);
    assert_eq!(next.node.as_deref(), Some("next"));
    t.select(Cursor::Node("root".into()));
    t.update(&key(KeyCode::Char('-')));
    assert!(!t.expanded("root") && !t.expanded("next"));
    let mut s = t.snapshot().clone();
    s.revision += 1;
    s.nodes.last_mut().unwrap().children = Children::Ready;
    assert!(t.resolve(&next, Outcome::Loaded(s)));
    assert!(!t.expanded("next"));
}
#[test]
fn actions_require_exact_single_use_results_and_keep_open_mark_on_failure() {
    let mut t = expanded();
    t.set_opened(Some("leaf")).unwrap();
    t.select(Cursor::Node("file".into()));
    t.update(&key(KeyCode::Enter));
    let r = request(&mut t);
    t.update(&key(KeyCode::Enter));
    assert_eq!(t.take_intents(), Vec::<Intent>::new());
    assert!(!t.resolve(&r, Outcome::Loaded(forest())));
    assert_eq!(t.opened(), Some("leaf"));
    assert!(t.resolve(&r, Outcome::Failed("changed on disk".into())));
    assert_eq!(t.opened(), Some("leaf"));
    t.update(&key(KeyCode::Enter));
    let newer = request(&mut t);
    assert!(!t.resolve(&r, Outcome::Opened));
    assert!(t.resolve(&newer, Outcome::Opened));
    assert_eq!(t.opened(), Some("file"));
    assert!(!t.resolve(&newer, Outcome::Opened));
    assert!(t.take_intents().contains(&Intent::Opened("file".into())));
}
#[test]
fn foreign_modified_removed_and_closed_requests_are_inert() {
    let mut t = expanded();
    let mut other = expanded();
    for tree in [&mut t, &mut other] {
        tree.select(Cursor::Node("file".into()));
        tree.update(&key(KeyCode::Enter));
    }
    let r = request(&mut t);
    let foreign = request(&mut other);
    assert!(!t.resolve(&foreign, Outcome::Opened));
    let mut s = t.snapshot().clone();
    s.revision += 1;
    s.nodes[2].revision += 1;
    s.nodes[2].name.push_str(" changed");
    t.replace(s).unwrap();
    assert!(!t.resolve(&r, Outcome::Opened));
    t.update(&key(KeyCode::Enter));
    let r = request(&mut t);
    let mut s = t.snapshot().clone();
    s.revision += 1;
    s.nodes.retain(|n| n.id != "file");
    t.replace(s).unwrap();
    assert!(!t.resolve(&r, Outcome::Opened));
    t.select(Cursor::Node("leaf".into()));
    t.update(&key(KeyCode::Enter));
    let r = request(&mut t);
    t.close();
    assert!(!t.resolve(&r, Outcome::Opened));
    assert_eq!(t.take_intents(), Vec::<Intent>::new());
}
#[test]
fn disabled_menu_choices_and_non_primary_actions_never_move_open_mark() {
    let mut s = forest();
    let mut disabled = Action::new("retry", "retry");
    disabled.disabled = Some("after completion".into());
    let mut custom = Action::new("copy", "copy path");
    custom.shortcut = Some('y');
    s.nodes[2].actions.extend([disabled, custom]);
    let mut t = TreeBrowser::new(s, Placement::FullScreen).unwrap();
    t.update(&key(KeyCode::Char('*')));
    t.take_intents();
    t.select(Cursor::Node("file".into()));
    t.update(&key(KeyCode::Char('a')));
    assert!(draw(&mut t, 64, 16, Presentation::default()).contains("after completion"));
    t.update(&key(KeyCode::Esc));
    t.update(&key(KeyCode::Char('y')));
    let r = request(&mut t);
    assert!(!t.resolve(&r, Outcome::Opened));
    let mut s = t.snapshot().clone();
    s.revision += 1;
    assert!(t.resolve(&r, Outcome::Applied(s)));
    assert!(t.opened().is_none());
}
#[test]
fn panel_focus_and_chat_opt_in_restore_owned_draft_without_consuming_other_input() {
    let mut t = TreeBrowser::new(forest(), Placement::Panel).unwrap();
    t.update(&key(KeyCode::Tab));
    assert!(!t.focused());
    assert_eq!(t.take_intents(), vec![Intent::MainView]);
    assert!(!t.update(&key(KeyCode::Down)));
    let mut chat = canonic_tui::chat::Chat::new();
    chat.set_input("draft");
    assert!(!chat.request_files());
    chat.set_files_available(true);
    assert!(chat.request_files() && chat.request_files());
    let i = chat.take_intents();
    assert_eq!(i.len(), 1);
    assert!(matches!(i[0], canonic_tui::chat::Intent::Files(_)));
    assert_eq!(chat.composer().editor().text(), "draft");
}
#[test]
fn release_repeat_modified_enter_and_paste_never_send_effects() {
    let mut t = expanded();
    t.select(Cursor::Node("file".into()));
    for kind in [KeyEventKind::Repeat, KeyEventKind::Release] {
        let mut k = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
        k.kind = kind;
        t.update(&Event::Key(k));
    }
    t.update(&Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::ALT,
    )));
    t.update(&Event::Paste("enter".into()));
    assert_eq!(t.take_intents(), Vec::<Intent>::new());
}
#[test]
fn rendering_all_profiles_tiny_long_deep_and_full_error_are_safe() {
    let mut t = expanded();
    t.select(Cursor::Node("file".into()));
    t.update(&key(KeyCode::Enter));
    let r = request(&mut t);
    t.resolve(
        &r,
        Outcome::Failed(format!("{} FINAL REASON", "failure ".repeat(30))),
    );
    for colors in [
        ColorMode::NoColor,
        ColorMode::Ansi16,
        ColorMode::Ansi256,
        ColorMode::TrueColor,
    ] {
        for symbols in [Symbols::Ascii, Symbols::Unicode] {
            for theme in [Theme::Dark, Theme::Light] {
                let p = Presentation {
                    colors,
                    symbols,
                    theme,
                };
                for (w, h) in [(1, 1), (20, 3), (40, 12), (64, 20)] {
                    let text = draw(&mut t, w, h, p);
                    assert!(!text.contains('\x1b'));
                }
            }
        }
    }
    for _ in 0..12 {
        t.update(&Event::Key(KeyEvent::new(
            KeyCode::PageDown,
            KeyModifiers::ALT,
        )));
    }
    assert!(draw(&mut t, 64, 20, Presentation::default()).contains("FINAL REASON"));
}
#[test]
fn tree_navigation_properties_never_select_removed_or_hidden_nodes() {
    use proptest::prelude::*;
    let mut runner = proptest::test_runner::TestRunner::default();
    runner
        .run(&proptest::collection::vec(0u8..8, 0..80), |keys| {
            let mut t = tree();
            for k in keys {
                let code = [
                    KeyCode::Up,
                    KeyCode::Down,
                    KeyCode::Left,
                    KeyCode::Right,
                    KeyCode::Char(' '),
                    KeyCode::Char('*'),
                    KeyCode::Char('-'),
                    KeyCode::End,
                ][usize::from(k)];
                t.update(&key(code));
                if let Some(Cursor::Node(id)) = t.cursor() {
                    prop_assert!(t.snapshot().nodes.iter().any(|n| n.id == *id));
                }
                let _ = draw(&mut t, 40, 12, Presentation::default());
            }
            Ok(())
        })
        .unwrap();
}

#[test]
fn host_child_refresh_and_root_refresh_invalidate_discovery_tickets() {
    let mut t = expanded();
    t.select(Cursor::Node("lazy".into()));
    t.update(&key(KeyCode::Right));
    let r = request(&mut t);
    let mut s = t.snapshot().clone();
    s.revision += 1;
    s.nodes[4].children = Children::Ready;
    t.replace(s).unwrap();
    assert!(!t.resolve(&r, Outcome::Failed("late".into())));
    let mut t = TreeBrowser::new(
        Snapshot {
            revision: 1,
            roots: Children::Unloaded,
            ..Snapshot::default()
        },
        Placement::FullScreen,
    )
    .unwrap();
    t.update(&key(KeyCode::Enter));
    let r = request(&mut t);
    let mut s = t.snapshot().clone();
    s.revision += 1;
    s.roots = Children::Ready;
    t.replace(s).unwrap();
    assert!(!t.resolve(&r, Outcome::Loaded(forest())));
}
#[test]
fn malformed_actions_ids_large_forests_and_deep_topology_are_bounded() {
    let mut s = forest();
    let duplicate = s.nodes[2].actions[0].clone();
    s.nodes[2].actions.push(duplicate);
    assert!(TreeBrowser::new(s, Placement::FullScreen).is_err());
    let mut s = forest();
    s.nodes[2].id = "control\nidentity".into();
    assert!(TreeBrowser::new(s, Placement::FullScreen).is_err());
    let mut s = forest();
    s.nodes = vec![s.nodes[2].clone(); 4097];
    assert!(TreeBrowser::new(s, Placement::FullScreen).is_err());
    let mut s = forest();
    for i in 0..280 {
        let parent = if i == 0 {
            "root".into()
        } else {
            format!("deep-{}", i - 1)
        };
        let mut n = Node::new(&format!("deep-{i}"), "nested", Some(&parent));
        n.has_children = true;
        s.nodes.push(n);
    }
    let mut t = TreeBrowser::new(s, Placement::FullScreen).unwrap();
    assert!(t.skipped() > 0);
    t.update(&key(KeyCode::Char('*')));
    let text = draw(&mut t, 40, 12, Presentation::default());
    assert!(text.contains("skipped"));
}
#[test]
fn pending_requests_are_bounded_and_independent_and_capacity_failure_is_visible() {
    let mut s = forest();
    let template = s.nodes[2].clone();
    for i in 0..300 {
        let mut n = template.clone();
        n.id = format!("pending-{i}");
        n.parent = None;
        s.nodes.push(n);
    }
    let mut t = TreeBrowser::new(s, Placement::FullScreen).unwrap();
    let mut requests = vec![];
    for i in 0..300 {
        t.select(Cursor::Node(format!("pending-{i}")));
        t.update(&key(KeyCode::Enter));
        requests.extend(t.take_intents().into_iter().filter_map(|i| {
            if let Intent::Request(r) = i {
                Some(r)
            } else {
                None
            }
        }));
    }
    assert_eq!(requests.len(), 256);
    assert!(draw(&mut t, 100, 16, Presentation::default()).contains("too many pending"));
    assert!(t.resolve(&requests[0], Outcome::Opened));
    assert!(!draw(&mut t, 100, 16, Presentation::default()).contains("too many pending"));
    t.take_intents();
    t.update(&key(KeyCode::Enter));
    assert_eq!(request(&mut t).node.as_deref(), Some("pending-299"));
}
#[test]
fn empty_loaded_children_and_reparented_nodes_have_valid_visible_selection() {
    let mut t = expanded();
    t.select(Cursor::Node("lazy".into()));
    t.update(&key(KeyCode::Right));
    let r = request(&mut t);
    let mut s = t.snapshot().clone();
    s.revision += 1;
    s.nodes[4].children = Children::Ready;
    assert!(t.resolve(&r, Outcome::Loaded(s)));
    assert!(draw(&mut t, 64, 16, Presentation::default()).contains("no children"));
    t.select(Cursor::Node("file".into()));
    t.select(Cursor::Node("lazy".into()));
    t.update(&key(KeyCode::Left));
    t.select(Cursor::Node("file".into()));
    let mut s = t.snapshot().clone();
    s.revision += 1;
    s.nodes[2].revision += 1;
    s.nodes[2].parent = Some("lazy".into());
    t.replace(s).unwrap();
    assert_ne!(node(&t), Some("file"));
    assert!(t.cursor().is_some());
}

#[test]
fn no_color_uses_reverse_open_underline_and_ascii_pending_chrome() {
    use canonic_tui::ratatui::style::{Color, Modifier};
    let mut t = expanded();
    t.select(Cursor::Node("file".into()));
    t.set_opened(Some("leaf")).unwrap();
    let mut terminal = Terminal::new(TestBackend::new(64, 16)).unwrap();
    let p = Presentation {
        colors: ColorMode::NoColor,
        symbols: Symbols::Ascii,
        theme: Theme::Dark,
    };
    terminal.draw(|f| t.render(f, f.area(), p)).unwrap();
    let buffer = terminal.backend().buffer();
    assert!(
        buffer
            .content
            .iter()
            .all(|c| c.fg == Color::Reset && c.bg == Color::Reset)
    );
    assert!(
        buffer
            .content
            .iter()
            .any(|c| c.modifier.contains(Modifier::REVERSED))
    );
    assert!(
        buffer
            .content
            .iter()
            .any(|c| c.modifier.contains(Modifier::UNDERLINED))
    );
    t.select(Cursor::Node("lazy".into()));
    t.update(&key(KeyCode::Right));
    assert!(draw(&mut t, 64, 16, p).is_ascii());
}

#[test]
fn authoritative_refresh_clears_old_load_and_action_errors() {
    let mut t = expanded();
    t.select(Cursor::Node("lazy".into()));
    t.update(&key(KeyCode::Right));
    let r = request(&mut t);
    t.resolve(&r, Outcome::Failed("OLD LOAD ERROR".into()));
    let mut s = t.snapshot().clone();
    s.revision += 1;
    s.nodes[4].children = Children::Ready;
    s.nodes.push(Node::new("fresh", "fresh.ts", Some("lazy")));
    t.replace(s).unwrap();
    assert!(!draw(&mut t, 64, 16, Presentation::default()).contains("OLD LOAD ERROR"));
    t.select(Cursor::Node("file".into()));
    t.update(&key(KeyCode::Enter));
    let r = request(&mut t);
    t.resolve(&r, Outcome::Failed("OLD ACTION ERROR".into()));
    let mut s = t.snapshot().clone();
    s.revision += 1;
    s.nodes[2].revision += 1;
    s.nodes[2].name = "new schedule.ts".into();
    t.replace(s).unwrap();
    assert!(!draw(&mut t, 64, 16, Presentation::default()).contains("OLD ACTION ERROR"));
    let mut t = TreeBrowser::new(
        Snapshot {
            revision: 1,
            roots: Children::Unloaded,
            ..Snapshot::default()
        },
        Placement::FullScreen,
    )
    .unwrap();
    t.update(&key(KeyCode::Enter));
    let r = request(&mut t);
    t.resolve(&r, Outcome::Failed("OLD ROOT ERROR".into()));
    t.replace(Snapshot {
        revision: 2,
        nodes: vec![Node::new("root", "loaded", None)],
        ..Snapshot::default()
    })
    .unwrap();
    assert!(!draw(&mut t, 64, 16, Presentation::default()).contains("OLD ROOT ERROR"));
}
#[test]
fn removed_child_retry_bookkeeping_resets_and_filter_control_navigation_stays_scoped() {
    let mut t = expanded();
    t.select(Cursor::Node("lazy".into()));
    t.update(&key(KeyCode::Right));
    let r = request(&mut t);
    assert!(t.pending(&r));
    let mut s = t.snapshot().clone();
    s.revision += 1;
    s.nodes.retain(|n| n.id != "lazy");
    t.replace(s).unwrap();
    assert!(!t.pending(&r));
    let mut s = t.snapshot().clone();
    s.revision += 1;
    s.nodes.push(forest().nodes[4].clone());
    t.replace(s).unwrap();
    t.select(Cursor::Node("lazy".into()));
    t.update(&key(KeyCode::Right));
    assert!(draw(&mut t, 64, 16, Presentation::default()).contains("attempt 1"));
    t.update(&key(KeyCode::Char('/')));
    t.update(&Event::Paste("schedule".into()));
    t.select(Cursor::Node("file".into()));
    t.update(&Event::Key(KeyEvent::new(
        KeyCode::Char('p'),
        KeyModifiers::CONTROL,
    )));
    assert_eq!(node(&t), Some("branch"));
    t.update(&Event::Key(KeyEvent::new(
        KeyCode::Char('n'),
        KeyModifiers::CONTROL,
    )));
    assert_eq!(node(&t), Some("file"));
}

#[test]
fn middle_cut_keeps_extensions_and_short_statuses_belong_to_each_row() {
    let mut s = forest();
    s.nodes[2].name = "very-long-schedule-file-name.graphql".into();
    s.nodes[2].state = canonic_tui::tree_browser::State::Modified;
    for i in 0..2 {
        let parent = if i == 0 {
            "branch".into()
        } else {
            format!("nested-{}", i - 1)
        };
        let mut n = Node::new(&format!("nested-{i}"), "nested", Some(&parent));
        n.has_children = true;
        s.nodes.push(n);
    }
    s.nodes[2].parent = Some("nested-1".into());
    let mut t = TreeBrowser::new(s, Placement::FullScreen).unwrap();
    t.update(&key(KeyCode::Char('*')));
    let text = draw(&mut t, 24, 16, Presentation::default());
    let line = text.lines().find(|line| line.contains(".graphql")).unwrap();
    assert!(line.trim_end().ends_with('M'), "{text}");
}

#[test]
fn paging_counts_physical_loading_and_error_rows() {
    let mut s = forest();
    for i in 0..5 {
        s.nodes
            .push(Node::new(&format!("extra-{i}"), "extra", None));
    }
    let mut t = TreeBrowser::new(s, Placement::FullScreen).unwrap();
    t.update(&key(KeyCode::Char('*')));
    t.take_intents();
    for id in ["file", "leaf"] {
        t.select(Cursor::Node(id.into()));
        t.update(&key(KeyCode::Enter));
        let r = request(&mut t);
        t.resolve(&r, Outcome::Failed("visible failure".into()));
    }
    t.select(Cursor::Node("file".into()));
    draw(&mut t, 64, 8, Presentation::default());
    t.update(&key(KeyCode::PageDown));
    assert_eq!(node(&t), Some("extra-0"));
    t.update(&key(KeyCode::PageUp));
    assert_eq!(node(&t), Some("file"));
}
#[test]
fn capacity_waiting_is_not_empty_and_only_delivered_loads_count_attempts() {
    let mut s = Snapshot {
        revision: 1,
        title: "files".into(),
        ..Snapshot::default()
    };
    let mut root = Node::new("root", "root", None);
    root.has_children = true;
    s.nodes.push(root);
    for i in 0..300 {
        let mut n = Node::new(&format!("folder-{i}"), "folder", Some("root"));
        n.has_children = true;
        n.children = Children::Unloaded;
        s.nodes.push(n);
    }
    let mut t = TreeBrowser::new(s, Placement::FullScreen).unwrap();
    t.update(&key(KeyCode::Char('*')));
    let requests: Vec<_> = t
        .take_intents()
        .into_iter()
        .filter_map(|i| {
            if let Intent::Request(r) = i {
                Some(r)
            } else {
                None
            }
        })
        .collect();
    assert_eq!(requests.len(), 256);
    assert!(t.select(Cursor::Children(Some("folder-299".into()))));
    assert!(draw(&mut t, 64, 12, Presentation::default()).contains("not loaded"));
    for r in requests.iter().take(44) {
        let mut s = t.snapshot().clone();
        s.revision += 1;
        s.nodes
            .iter_mut()
            .find(|n| Some(&n.id) == r.node.as_ref())
            .unwrap()
            .children = Children::Ready;
        assert!(t.resolve(r, Outcome::Loaded(s)));
        t.take_intents();
    }
    let text = draw(&mut t, 64, 12, Presentation::default());
    assert!(
        text.contains("loading") && text.contains("attempt 1"),
        "{text}"
    );
}
#[test]
fn manually_collapsing_a_bulk_expanded_child_stops_overlapping_cascades() {
    let mut t = tree();
    t.update(&key(KeyCode::Char('*')));
    let r = request(&mut t);
    t.select(Cursor::Node("lazy".into()));
    t.update(&key(KeyCode::Left));
    let mut s = t.snapshot().clone();
    s.revision += 1;
    s.nodes[4].children = Children::Ready;
    let mut child = Node::new("next", "next", Some("lazy"));
    child.has_children = true;
    child.children = Children::Unloaded;
    s.nodes.push(child);
    assert!(t.resolve(&r, Outcome::Loaded(s)));
    assert!(!t.expanded("lazy") && !t.expanded("next"));
    assert_eq!(t.take_intents(), Vec::<Intent>::new());
}
