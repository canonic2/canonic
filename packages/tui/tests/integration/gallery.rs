//! Verifies gallery navigation, fixture rendering, headless output and PTY lifecycle.

use assert_cmd::Command;
use canonic_tui::{
    event::{Event, KeyCode, KeyEvent, KeyModifiers},
    gallery::{Component, Gallery},
    ratatui::{Terminal, backend::TestBackend},
};
use predicates::prelude::*;

fn key(code: KeyCode) -> Event {
    Event::Key(KeyEvent::new(code, KeyModifiers::NONE))
}

#[test]
fn scenario_browser_preserves_draft_on_cancel_and_opens_live_workflows() {
    let mut g = Gallery::new();
    g.open_scenario(Component::Composer, "main").unwrap();
    g.update(&Event::Paste("Unsent draft".into()));
    g.update(&key(KeyCode::Left));
    g.update(&key(KeyCode::Left));
    g.update(&key(KeyCode::F(10)));
    assert_eq!(g.current(), "scenarios");
    g.update(&key(KeyCode::F(10)));
    assert_eq!(g.current(), "composer");
    assert_eq!(g.composer_input(0), Some("Unsent draft"));
    g.update(&Event::Paste("!".into()));
    assert_eq!(g.composer_input(0), Some("Unsent dra!ft"));
    g.update(&key(KeyCode::F(10)));
    g.update(&Event::Paste("up-key".into()));
    g.update(&key(KeyCode::Enter));
    assert_eq!(g.current(), "composer");
    let mut t = Terminal::new(TestBackend::new(80, 24)).unwrap();
    t.draw(|f| g.render(f)).unwrap();
    let text = super::components::dump(t.backend().buffer());
    assert!(text.contains("message"));
    assert!(text.contains("and bump the app version"));
    g.update(&key(KeyCode::F(10)));
    for _ in 0..6 {
        g.update(&key(KeyCode::Backspace));
    }
    g.update(&Event::Paste("sending".into()));
    g.update(&key(KeyCode::Enter));
    t.draw(|f| g.render(f)).unwrap();
    let text = super::components::dump(t.backend().buffer());
    assert!(text.contains("you · 20:10 · sending"));
    assert!(text.contains("Run the digest tests too."));
    assert_eq!(g.composer_input(0), Some(""));
}

fn picker_screen(gallery: &mut Gallery) -> String {
    let mut terminal = Terminal::new(TestBackend::new(80, 12)).unwrap();
    terminal.draw(|frame| gallery.render(frame)).unwrap();
    super::components::dump(terminal.backend().buffer())
}

#[test]
fn box_mode_fixture_escape_restores_draft_before_leaving_scenario() {
    let mut gallery = Gallery::new();
    gallery
        .open_scenario(Component::Composer, "box-history")
        .unwrap();
    let recalled = picker_screen(&mut gallery);
    assert!(recalled.contains("history ┄"));
    assert!(recalled.contains("3 of 12"));
    gallery.update(&key(KeyCode::Esc));
    assert_eq!(gallery.current(), "composer");
    assert_eq!(
        gallery.composer_input(0),
        Some("and bump the app version\nthen tag a release candidate")
    );
    gallery.update(&Event::Paste("!".into()));
    assert_eq!(
        gallery.composer_input(0),
        Some("and bump the app version\nthen tag a release candidat!e")
    );
    gallery.update(&key(KeyCode::F(10)));
    assert_eq!(gallery.current(), "scenarios");
}

#[test]
fn box_history_scenario_browses_real_entries_and_down_restores_the_original_cursor() {
    let mut gallery = Gallery::new();
    gallery
        .open_scenario(Component::Composer, "box-history")
        .unwrap();
    gallery.update(&key(KeyCode::Up));
    assert!(picker_screen(&mut gallery).contains("4 of 12"));
    assert!(gallery.composer_input(0).unwrap().contains("2026-03-29"));
    gallery.update(&key(KeyCode::Down));
    assert!(picker_screen(&mut gallery).contains("3 of 12"));
    for _ in 0..3 {
        gallery.update(&key(KeyCode::Down));
    }
    assert_eq!(gallery.current(), "composer");
    assert_eq!(
        gallery.composer_input(0),
        Some("and bump the app version\nthen tag a release candidate")
    );
    gallery.update(&Event::Paste("!".into()));
    assert_eq!(
        gallery.composer_input(0),
        Some("and bump the app version\nthen tag a release candidat!e")
    );
}

#[test]
fn escape_restores_both_picker_viewports_and_component_specific_searches() {
    let mut g = Gallery::new();
    for _ in 0..5 {
        g.update(&key(KeyCode::Down));
    }
    picker_screen(&mut g);
    g.update(&key(KeyCode::Up));
    let catalog = picker_screen(&mut g);
    assert!(catalog.contains("> Commands and bindings"));
    g.update(&key(KeyCode::Enter));
    for _ in 0..10 {
        g.update(&key(KeyCode::Down));
    }
    picker_screen(&mut g);
    g.update(&key(KeyCode::Up));
    let commands = picker_screen(&mut g);
    assert!(commands.contains("> pending-hints"));
    g.update(&key(KeyCode::Enter));
    // The command dispatcher consumes Escape to cancel its pending sequence first.
    for _ in 0..3 {
        if g.current() == "scenarios" {
            break;
        }
        g.update(&key(KeyCode::Esc));
    }
    assert_eq!(g.current(), "scenarios");
    assert_eq!(picker_screen(&mut g), commands);
    g.update(&key(KeyCode::Esc));
    assert_eq!(picker_screen(&mut g), catalog);
    g.update(&key(KeyCode::Down));
    g.update(&key(KeyCode::Enter));
    g.update(&Event::Paste("history".into()));
    g.update(&key(KeyCode::Down));
    let composer = picker_screen(&mut g);
    assert!(composer.contains("Search: history"));
    assert!(composer.contains("> history-search"));
    g.update(&key(KeyCode::Enter));
    assert_eq!(g.current(), "composer");
    g.update(&key(KeyCode::Esc)); // close history search
    g.update(&key(KeyCode::F(10)));
    assert_eq!(picker_screen(&mut g), composer);
    g.update(&key(KeyCode::Esc));
    g.update(&key(KeyCode::Up));
    g.update(&key(KeyCode::Enter));
    assert_eq!(picker_screen(&mut g), commands);
    g.update(&key(KeyCode::Esc));
    g.update(&key(KeyCode::Down));
    g.update(&key(KeyCode::Enter));
    assert_eq!(picker_screen(&mut g), composer);
}

#[test]
fn escape_retains_the_component_picker_search_and_selection() {
    let mut g = Gallery::new();
    g.update(&Event::Paste("editor".into()));
    let before = picker_screen(&mut g);
    assert!(before.contains("Search: editor"));
    g.update(&key(KeyCode::Enter));
    assert_eq!(g.current(), "scenarios");
    g.update(&key(KeyCode::Esc));
    assert_eq!(picker_screen(&mut g), before);
    g.update(&key(KeyCode::Enter));
    assert_eq!(g.current(), "scenarios");
}

#[test]
fn named_lifecycle_fixtures_project_real_public_behavior() {
    let mut g = Gallery::new();
    let mut t = Terminal::new(TestBackend::new(100, 28)).unwrap();
    for (id, draft, pending) in [
        ("sending", "", true),
        ("accepted", "", false),
        ("refused", "Run the digest tests too.", false),
        ("refused-kept", "And bump the app version.", false),
    ] {
        g.open_scenario(Component::Composer, id).unwrap();
        assert_eq!(g.composer_input(0), Some(draft));
        t.draw(|f| g.render(f)).unwrap();
        let text = super::components::dump(t.backend().buffer());
        assert_eq!(text.contains("20:10 · sending"), pending);
    }
    g.open_scenario(Component::Dialog, "replace").unwrap();
    t.draw(|f| g.render(f)).unwrap();
    assert!(super::components::dump(t.backend().buffer()).contains("Replacement prompt"));
    g.open_scenario(Component::Dialog, "late").unwrap();
    t.draw(|f| g.render(f)).unwrap();
    assert!(super::components::dump(t.backend().buffer()).contains("Rejected stale result"));
    g.open_scenario(Component::Dialog, "remove").unwrap();
    assert_eq!(g.dialog_depth(), 0);
}

#[test]
fn scenario_profiles_bound_the_stage_and_use_requested_color_and_symbols() {
    use canonic_tui::ratatui::style::Color;
    let mut g = Gallery::new();
    let mut t = Terminal::new(TestBackend::new(100, 28)).unwrap();
    g.open_scenario(Component::Editor, "narrow-40").unwrap();
    t.draw(|f| g.render(f)).unwrap();
    for y in 2..26 {
        for x in 40..100 {
            assert_eq!(t.backend().buffer()[(x, y)].symbol(), " ");
        }
    }
    g.open_scenario(Component::Editor, "ansi16").unwrap();
    t.draw(|f| g.render(f)).unwrap();
    assert!(
        t.backend()
            .buffer()
            .content
            .iter()
            .any(|cell| cell.fg == Color::Cyan)
    );
    g.open_scenario(Component::Editor, "no-color").unwrap();
    t.draw(|f| g.render(f)).unwrap();
    assert!(
        t.backend()
            .buffer()
            .content
            .iter()
            .all(|cell| cell.fg == Color::Reset && cell.bg == Color::Reset)
    );
    g.open_scenario(Component::Editor, "ascii").unwrap();
    t.draw(|f| g.render(f)).unwrap();
    let text = super::components::dump(t.backend().buffer());
    assert!(!text.contains('─'));
    assert!(text.contains("draft ---"));
}

#[test]
fn completion_states_keep_the_dock_fixed_and_argument_hints_out_of_the_draft() {
    let mut gallery = Gallery::new();
    let mut terminal = Terminal::new(TestBackend::new(64, 28)).unwrap();
    let mut dock = None;
    for id in [
        "main",
        "files",
        "loading",
        "still-loading",
        "no-match-command",
        "no-match-file",
        "error",
        "partly-failed",
        "disabled",
        "command-inserted",
        "file-inserted",
        "narrow",
        "long-description",
        "mixed-sources",
        "doesnt-open",
    ] {
        gallery
            .open_scenario(Component::CompletionMenu, id)
            .unwrap();
        terminal.draw(|f| gallery.render(f)).unwrap();
        let text = super::components::dump(terminal.backend().buffer());
        let message = text
            .lines()
            .position(|line| line.starts_with("message "))
            .unwrap();
        assert_eq!(*dock.get_or_insert(message), message, "{id}");
        if id == "command-inserted" {
            assert!(text.contains("/review <pr>"));
            assert_eq!(gallery.completion_input(), "/review ");
        }
        if matches!(id, "main" | "narrow" | "long-description" | "partly-failed") {
            insta::assert_snapshot!(format!("completion_{id}"), text);
        }
    }
    gallery
        .open_scenario(Component::CompletionMenu, "main")
        .unwrap();
    gallery.update(&key(KeyCode::Esc));
    assert_eq!(gallery.current(), "completion-menu");
    assert_eq!(gallery.completion_input(), "/");
    gallery.update(&key(KeyCode::Esc));
    assert_eq!(gallery.current(), "scenarios");
}

#[test]
fn listed_scenarios_open_and_render_at_small_sizes_without_a_terminal() {
    let output = Command::new(assert_cmd::cargo::cargo_bin!("tui-gallery"))
        .arg("--list")
        .output()
        .unwrap();
    assert!(output.status.success());
    let text = String::from_utf8(output.stdout).unwrap();
    let mut g = Gallery::new();
    for (name, component) in [
        ("picker", Component::Picker),
        ("chat", Component::Chat),
        ("editor", Component::Editor),
        ("dialog", Component::Dialog),
        ("commands", Component::Commands),
        ("composer", Component::Composer),
        ("completion-menu", Component::CompletionMenu),
    ] {
        let section = text.split(&format!("\n{name} scenarios (")).nth(1).unwrap();
        for line in section
            .lines()
            .skip(1)
            .take_while(|line| line.starts_with("  "))
        {
            let id = line.split_whitespace().next().unwrap();
            g.open_scenario(component, id).unwrap();
            for (width, height) in [(0, 0), (20, 8), (64, 24)] {
                let mut t = Terminal::new(TestBackend::new(width, height)).unwrap();
                t.draw(|f| g.render(f)).unwrap();
            }
        }
    }
    g.open_scenario(Component::Composer, "main").unwrap();
    g.update(&Event::Paste("Keep me".into()));
    assert!(g.open_scenario(Component::Composer, "missing").is_err());
    assert_eq!(g.composer_input(0), Some("Keep me"));
    Command::new(assert_cmd::cargo::cargo_bin!("tui-gallery"))
        .args(["--component", "composer", "--scenario", "missing"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("Unknown composer scenario"))
        .stdout(predicate::str::contains("\x1b").not());
}

#[test]
fn draft_fixture_delays_responses_preserves_new_edits_and_keeps_the_dock_stable() {
    let mut gallery = Gallery::new();
    gallery.open_scenario(Component::Composer, "main").unwrap();
    assert_eq!(gallery.current(), "composer");
    gallery.update(&Event::Paste("original".into()));
    gallery.update(&key(KeyCode::Enter));
    gallery.update(&key(KeyCode::Enter));
    let mut terminal = Terminal::new(TestBackend::new(100, 28)).unwrap();
    terminal.draw(|f| gallery.render(f)).unwrap();
    let pending = super::components::dump(terminal.backend().buffer());
    assert!(pending.contains("1 submission intents"));
    assert!(pending.contains("you · 20:10 · sending"));
    assert_eq!(gallery.composer_input(0), Some(""));
    assert!(!pending.contains("Submission snapshot"));
    assert!(!pending.contains("Draft 1"));
    let message_row = pending
        .lines()
        .position(|r| r.starts_with("message "))
        .unwrap();
    gallery.update(&Event::Paste(" newer".into()));
    gallery.update(&key(KeyCode::Tab));
    assert_eq!(gallery.composer_input(0), Some(" newer"));
    gallery.update(&key(KeyCode::F(5)));
    terminal.draw(|f| gallery.render(f)).unwrap();
    let accepted = super::components::dump(terminal.backend().buffer());
    assert!(!accepted.contains("20:10 · sending"));
    assert!(accepted.contains("you · 20:10"));
    assert_eq!(
        accepted.lines().position(|r| r.starts_with("message ")),
        Some(message_row)
    );
    assert_eq!(gallery.composer_input(1), None);
    gallery.update(&key(KeyCode::F(5))); // finish accepted turn
    gallery.update(&key(KeyCode::Enter));
    gallery.update(&key(KeyCode::F(6)));
    gallery.update(&Event::Paste("retry".into()));
    gallery.update(&key(KeyCode::Enter));
    gallery.update(&key(KeyCode::F(8)));
    terminal.draw(|f| gallery.render(f)).unwrap();
    let stale = super::components::dump(terminal.backend().buffer());
    assert!(stale.contains("Stale response ignored"));
    assert!(stale.contains("you · 20:10 · sending"));
    assert!(stale.contains("3 submission intents"));
    gallery.update(&key(KeyCode::F(7)));
    gallery.update(&key(KeyCode::F(8)));
    assert_eq!(gallery.composer_input(0), Some("Replacement draft"));
    for (width, height) in [(40, 12), (1, 1), (0, 0)] {
        let mut tiny = Terminal::new(TestBackend::new(width, height)).unwrap();
        tiny.draw(|f| gallery.render(f)).unwrap();
    }
    gallery.update(&key(KeyCode::Esc));
    gallery.update(&key(KeyCode::Enter));
    assert_eq!(gallery.composer_input(0), Some(""));
    assert_eq!(gallery.composer_input(1), None);
}

#[test]
fn command_fixture_preserves_draft_and_replaces_effective_help_bindings() {
    let mut gallery = Gallery::new();
    gallery.open_scenario(Component::Commands, "main").unwrap();
    assert_eq!(gallery.current(), "commands");
    let original = gallery.command_input().to_owned();
    gallery.update(&Event::Paste("/branch".into()));
    gallery.update(&key(KeyCode::Enter));
    assert_eq!(gallery.command_input(), original);
    let mut terminal = Terminal::new(TestBackend::new(100, 25)).unwrap();
    terminal.draw(|f| gallery.render(f)).unwrap();
    assert!(super::components::dump(terminal.backend().buffer()).contains("intent: rename"));
    gallery.update(&key(KeyCode::F(5)));
    gallery.update(&key(KeyCode::F(3)));
    terminal.draw(|f| gallery.render(f)).unwrap();
    let text = super::components::dump(terminal.backend().buffer());
    assert!(
        text.lines().any(|r| r.starts_with("keys ")) && text.contains("ctrl+o"),
        "{text}"
    );
    gallery.update(&key(KeyCode::F(6)));
    gallery.update(&Event::Key(KeyEvent::new(
        KeyCode::Char('x'),
        KeyModifiers::CONTROL,
    )));
    gallery.update(&Event::Paste("must not leak".into()));
    assert_eq!(gallery.command_input(), original);
    for (width, height) in [(40, 12), (1, 1), (80, 20)] {
        terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal.draw(|f| gallery.render(f)).unwrap();
    }
}

#[test]
fn commands_dock_and_keys_fill_the_stage_without_losing_the_message_box() {
    for (width, height) in [(100, 30), (40, 30)] {
        let mut gallery = Gallery::new();
        gallery.open_scenario(Component::Commands, "main").unwrap();
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        let mut expected_title = None;
        for _ in 0..3 {
            terminal.draw(|f| gallery.render(f)).unwrap();
            let text = super::components::dump(terminal.backend().buffer());
            let rows: Vec<_> = text.lines().collect();
            let message = rows
                .iter()
                .position(|r| r.starts_with("message "))
                .unwrap_or_else(|| panic!("{text}"));
            let title = rows
                .iter()
                .position(|r| r.starts_with("commands "))
                .unwrap();
            assert!(title > 2, "view should dock below the gallery header");
            assert_eq!(
                *expected_title.get_or_insert(title),
                title,
                "filter fixtures keep their opening height"
            );
            assert!(
                rows[message - 2].contains("esc"),
                "picker footer must touch activity row"
            );
            gallery.update(&key(KeyCode::F(3)));
            terminal.draw(|f| gallery.render(f)).unwrap();
            let help = super::components::dump(terminal.backend().buffer());
            assert!(help.lines().nth(2).unwrap().starts_with("keys "));
            assert!(!help.lines().any(|r| r.starts_with("message ")));
            gallery.update(&key(KeyCode::F(6)));
            terminal.draw(|f| gallery.render(f)).unwrap();
            let closed = super::components::dump(terminal.backend().buffer());
            assert_eq!(
                closed.lines().position(|r| r.starts_with("message ")),
                Some(message)
            );
            gallery.update(&key(KeyCode::F(2)));
        }
    }
}

#[test]
fn command_design_palette_and_keys_have_distinct_reviewable_projections() {
    let mut gallery = Gallery::new();
    gallery.open_scenario(Component::Commands, "main").unwrap();
    let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
    terminal.draw(|f| gallery.render(f)).unwrap();
    insta::assert_snapshot!(
        "command_palette",
        super::components::dump(terminal.backend().buffer())
    );
    gallery.update(&key(KeyCode::F(1)));
    terminal.draw(|f| gallery.render(f)).unwrap();
    insta::assert_snapshot!(
        "command_keys",
        super::components::dump(terminal.backend().buffer())
    );
    gallery.update(&key(KeyCode::Esc));
    let original = gallery.command_input().to_owned();
    gallery.update(&Event::Key(KeyEvent::new(
        KeyCode::Char('x'),
        KeyModifiers::CONTROL,
    )));
    for _ in 0..4 {
        gallery.tick();
    }
    terminal.draw(|f| gallery.render(f)).unwrap();
    let before = super::components::dump(terminal.backend().buffer());
    assert!(before.contains("ctrl+x …") && !before.contains("copy handoff"));
    gallery.tick();
    terminal.draw(|f| gallery.render(f)).unwrap();
    assert!(super::components::dump(terminal.backend().buffer()).contains("h  copy handoff"));
    gallery.update(&key(KeyCode::Char('z')));
    terminal.draw(|f| gallery.render(f)).unwrap();
    assert!(super::components::dump(terminal.backend().buffer()).contains("isn't a command"));
    assert_eq!(gallery.command_input(), original);
    for _ in 0..38 {
        gallery.tick();
    }
    terminal.draw(|f| gallery.render(f)).unwrap();
    assert!(!super::components::dump(terminal.backend().buffer()).contains("isn't a command"));
    gallery.update(&Event::Key(KeyEvent::new(
        KeyCode::Char('x'),
        KeyModifiers::CONTROL,
    )));
    gallery.update(&key(KeyCode::Char('k')));
    gallery.update(&Event::Key(KeyEvent::new(
        KeyCode::Char('p'),
        KeyModifiers::CONTROL,
    )));
    terminal.draw(|f| gallery.render(f)).unwrap();
    let recent = super::components::dump(terminal.backend().buffer());
    assert!(recent.find("stop session").unwrap() < recent.find("copy handoff").unwrap());
}

#[test]
fn gallery_opens_at_the_selector_and_both_components_are_independent_fixtures() {
    let mut gallery = Gallery::new();
    assert_eq!(gallery.current(), "catalog");
    let mut terminal = Terminal::new(TestBackend::new(64, 16)).unwrap();
    terminal.draw(|frame| gallery.render(frame)).unwrap();
    let text = super::components::dump(terminal.backend().buffer());
    assert!(text.contains("Option picker") && text.contains("Full-screen chat"));
    insta::assert_snapshot!("component_selector", text);
    gallery.update(&key(KeyCode::Enter));
    assert_eq!(gallery.current(), "scenarios");
    terminal.draw(|frame| gallery.render(frame)).unwrap();
    assert!(super::components::dump(terminal.backend().buffer()).contains("main · interactive"));
    gallery.update(&key(KeyCode::Enter));
    assert_eq!(gallery.current(), "picker");
    gallery.update(&key(KeyCode::Enter));
    terminal.draw(|frame| gallery.render(frame)).unwrap();
    assert!(super::components::dump(terminal.backend().buffer()).contains("Selected skip"));
    gallery.update(&key(KeyCode::Esc));
    assert_eq!(gallery.current(), "scenarios");
    gallery.update(&key(KeyCode::Esc));
    assert_eq!(gallery.current(), "catalog");
    gallery.update(&key(KeyCode::Down));
    gallery.update(&key(KeyCode::Enter));
    assert_eq!(gallery.current(), "scenarios");
    gallery.update(&key(KeyCode::Enter));
    assert_eq!(gallery.current(), "chat");
    gallery.update(&Event::Paste("hello".into()));
    gallery.update(&key(KeyCode::Enter));
    for _ in 0..40 {
        gallery.tick();
    }
    terminal = Terminal::new(TestBackend::new(100, 25)).unwrap();
    terminal.draw(|frame| gallery.render(frame)).unwrap();
    let text = super::components::dump(terminal.backend().buffer());
    assert!(text.contains("Simulated response:") && text.contains("hello"));
    for _ in 0..6 {
        gallery.update(&key(KeyCode::F(2)));
        terminal.draw(|frame| gallery.render(frame)).unwrap();
    }
    gallery.update(&key(KeyCode::Esc));
    assert_eq!(gallery.current(), "scenarios");
    gallery.update(&key(KeyCode::Esc));
    assert_eq!(gallery.current(), "catalog");
}

#[test]
fn editor_fixture_routes_paste_to_focused_scope_and_resets_on_reopen() {
    let mut gallery = Gallery::new();
    gallery.open_scenario(Component::Editor, "main").unwrap();
    assert_eq!(gallery.current(), "editor");
    let first = gallery.editor_input(0).unwrap().to_owned();
    assert_eq!(first.lines().count(), 2);
    let mut terminal = Terminal::new(TestBackend::new(100, 25)).unwrap();
    terminal.draw(|frame| gallery.render(frame)).unwrap();
    let rendered = super::components::dump(terminal.backend().buffer());
    assert!(
        rendered.contains("Edit this draft in the middle.\nEnter or Ctrl-J inserts a newline.")
    );
    gallery.update(&key(KeyCode::Tab));
    gallery.update(&Event::Paste(" changed".into()));
    assert_eq!(gallery.editor_input(0), Some(first.as_str()));
    let second = gallery.editor_input(1).unwrap().to_owned();
    gallery.update(&key(KeyCode::F(3)));
    gallery.update(&Event::Paste("modal".into()));
    gallery.update(&key(KeyCode::Tab));
    gallery.update(&Event::Paste(" only".into()));
    assert_eq!(gallery.editor_input(0), Some(first.as_str()));
    assert_eq!(gallery.editor_input(1), Some(second.as_str()));
    assert_eq!(gallery.editor_input(2), Some("modal only"));
    gallery.update(&Event::Key(KeyEvent::new(
        KeyCode::Char('g'),
        KeyModifiers::CONTROL,
    )));
    gallery.update(&key(KeyCode::Esc));
    gallery.update(&Event::Paste("!".into()));
    assert_eq!(gallery.editor_input(2), Some("modal only!"));
    assert_eq!(gallery.editor_input(1), Some(second.as_str()));
    gallery.update(&key(KeyCode::Esc));
    assert_eq!(gallery.current(), "editor");
    gallery.update(&Event::Paste(" restored".into()));
    assert_eq!(
        gallery.editor_input(1),
        Some(format!("{second} restored").as_str())
    );
    gallery.update(&key(KeyCode::Esc));
    assert_eq!(gallery.current(), "scenarios");
    gallery.update(&key(KeyCode::Enter));
    assert_eq!(gallery.current(), "editor");
    assert_eq!(gallery.editor_input(1), Some("Independent second draft"));
    for scenario in 0..12 {
        let draft = gallery.editor_input(0).unwrap();
        assert!(!draft.contains("\\n"));
        if scenario == 5 {
            assert!(draft.contains("Cafe\u{301}"));
        }
        gallery.update(&key(KeyCode::F(2)));
    }
    assert!(gallery.update(&Event::Key(KeyEvent::new(
        KeyCode::Char('c'),
        KeyModifiers::CONTROL
    ))));
}

#[test]
fn rich_picker_fixtures_isolate_the_host_draft_and_reset_after_confirmation() {
    let mut gallery = Gallery::new();
    gallery.open_scenario(Component::Picker, "main").unwrap();
    let original = gallery.picker_input().to_owned();
    gallery.update(&Event::Paste("ignored without a filter".into()));
    assert_eq!(gallery.picker_input(), original);
    gallery.update(&key(KeyCode::Tab));
    gallery.update(&Event::Paste(" retained".into()));
    let draft = gallery.picker_input().to_owned();
    gallery.update(&key(KeyCode::Tab));
    let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
    for _ in 0..14 {
        terminal.draw(|f| gallery.render(f)).unwrap();
        assert_eq!(gallery.picker_input(), draft);
        gallery.update(&key(KeyCode::F(2)));
    }
    for _ in 0..4 {
        gallery.update(&key(KeyCode::F(2)));
    }
    gallery.update(&key(KeyCode::Esc));
    assert_eq!(gallery.current(), "picker");
    gallery.update(&key(KeyCode::Esc));
    assert_eq!(gallery.current(), "scenarios");
    gallery.update(&key(KeyCode::Enter));
    assert_eq!(gallery.picker_input(), original);
    gallery.update(&key(KeyCode::Char('1')));
    terminal.draw(|f| gallery.render(f)).unwrap();
    assert!(super::components::dump(terminal.backend().buffer()).contains("Selected once"));
}

#[test]
fn headless_listing_help_and_refusal_do_not_change_terminal_modes() {
    Command::new(assert_cmd::cargo::cargo_bin!("tui-gallery"))
        .arg("--list")
        .assert()
        .success()
        .stdout(predicate::str::contains("Full-screen chat"));
    Command::new(assert_cmd::cargo::cargo_bin!("tui-gallery"))
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("--component"));
    Command::new(assert_cmd::cargo::cargo_bin!("tui-gallery"))
        .assert()
        .failure()
        .stderr(predicate::str::contains("interactive terminal"))
        .stdout(predicate::str::contains("\x1b").not());
}

#[test]
fn picker_dock_reserves_activity_across_fixtures_and_confirmation() {
    let mut g = Gallery::new();
    g.open_scenario(Component::Picker, "main").unwrap();
    let mut t = Terminal::new(TestBackend::new(100, 28)).unwrap();
    let mut message_y = None;
    for step in 0..14 {
        t.draw(|f| g.render(f)).unwrap();
        let text = super::components::dump(t.backend().buffer());
        let lines: Vec<_> = text.lines().collect();
        let message = lines
            .iter()
            .position(|line| line.starts_with("message "))
            .unwrap();
        assert_ne!(lines[message - 1].trim(), "");
        assert_eq!(*message_y.get_or_insert(message), message);
        if step == 0 {
            assert!(lines[message - 1].starts_with("! needs you · 1–3"));
            assert!(lines[message - 2].contains("enter"));
            assert!(lines[message - 2].contains("1–3"));
            let presentation = canonic_tui::theme::Presentation::default();
            let buffer = t.backend().buffer();
            let activity = u16::try_from(message - 1).unwrap();
            assert_eq!(
                buffer[(0, activity)].fg,
                presentation.tokens().warning.unwrap()
            );
            assert_eq!(
                buffer[(13, activity)].fg,
                canonic_tui::ratatui::style::Color::Reset
            );
            let title = lines
                .iter()
                .position(|line| line.starts_with("github server "))
                .unwrap();
            assert_eq!(message - title, 6);
        }
        g.update(&key(KeyCode::F(2)));
    }
    g.update(&key(KeyCode::Enter));
    t.draw(|f| g.render(f)).unwrap();
    let text = super::components::dump(t.backend().buffer());
    let lines: Vec<_> = text.lines().collect();
    let message = lines
        .iter()
        .position(|line| line.starts_with("message "))
        .unwrap();
    assert_eq!(Some(message), message_y);
    assert_eq!(lines[message - 1].trim(), "ready");
}

#[test]
fn dialog_fixtures_preserve_host_drafts_and_reject_replaced_results() {
    let mut gallery = Gallery::new();
    let mut terminal = Terminal::new(TestBackend::new(100, 28)).unwrap();
    gallery.open_scenario(Component::Dialog, "main").unwrap();
    assert_eq!(gallery.current(), "dialog");
    assert_eq!(gallery.dialog_depth(), 1);
    let first = gallery.dialog_input(0).unwrap().to_owned();
    let second = gallery.dialog_input(1).unwrap().to_owned();
    gallery.update(&Event::Paste("MODAL ONLY".into()));
    gallery.update(&key(KeyCode::F(3)));
    assert_eq!(gallery.dialog_depth(), 2);
    gallery.update(&Event::Paste("CHILD ONLY".into()));
    gallery.update(&key(KeyCode::Esc));
    assert_eq!(gallery.dialog_depth(), 1);
    gallery.update(&key(KeyCode::F(6)));
    gallery.update(&key(KeyCode::F(7)));
    terminal.draw(|f| gallery.render(f)).unwrap();
    assert!(super::components::dump(terminal.backend().buffer()).contains("Rejected stale result"));
    assert_eq!(gallery.dialog_input(0), Some(first.as_str()));
    assert_eq!(gallery.dialog_input(1), Some(second.as_str()));
    gallery.update(&key(KeyCode::F(8)));
    assert_eq!(gallery.dialog_depth(), 0);
    gallery.update(&Event::Paste(" BASE ONLY".into()));
    assert!(gallery.dialog_input(0).unwrap().ends_with(" BASE ONLY"));
    // Removed return target repairs focus to the surviving second draft.
    for _ in 0..5 {
        gallery.update(&key(KeyCode::F(2)));
    }
    gallery.update(&key(KeyCode::Esc));
    gallery.update(&Event::Paste(" SECOND ONLY".into()));
    assert!(gallery.dialog_input(1).unwrap().ends_with(" SECOND ONLY"));
    gallery.update(&key(KeyCode::Esc));
    assert_eq!(gallery.current(), "scenarios");
    gallery.update(&key(KeyCode::Enter));
    assert_eq!(gallery.dialog_input(0), Some(first.as_str()));
    // Long inspection scrolls within its body; Enter acknowledges only that layer.
    gallery.update(&key(KeyCode::F(2)));
    gallery.update(&key(KeyCode::F(2)));
    gallery.update(&key(KeyCode::End));
    terminal.draw(|f| gallery.render(f)).unwrap();
    assert!(super::components::dump(terminal.backend().buffer()).contains("Inspection row 59"));
    gallery.update(&key(KeyCode::Enter));
    assert_eq!(gallery.dialog_depth(), 0);
}

#[test]
fn dialog_panel_keeps_dock_position_across_growth_nesting_and_replacement() {
    let mut g = Gallery::new();
    g.open_scenario(Component::Dialog, "main").unwrap();
    // Default ANSI 16 -> indexed fill.
    g.update(&key(KeyCode::F(4)));
    let mut t = Terminal::new(TestBackend::new(100, 40)).unwrap();
    let mut message_y = None;
    for step in 0..6 {
        t.draw(|f| g.render(f)).unwrap();
        let text = super::components::dump(t.backend().buffer());
        let lines: Vec<_> = text.lines().collect();
        let message = lines
            .iter()
            .position(|line| line.starts_with("message "))
            .unwrap();
        assert_eq!(*message_y.get_or_insert(message), message);
        assert!(lines[message - 1].starts_with(if step == 5 { "ready" } else { "! dialog" }));
        if step == 0 {
            let p = canonic_tui::theme::Presentation {
                colors: canonic_tui::theme::ColorMode::Ansi256,
                ..Default::default()
            };
            let activity_y = u16::try_from(message - 1).unwrap();
            let buffer = t.backend().buffer();
            assert_eq!(buffer[(0, activity_y)].fg, p.tokens().warning.unwrap());
            assert_eq!(buffer[(8, activity_y)].fg, p.tokens().dim.unwrap());
            assert_eq!(buffer[(0, 3)].fg, p.tokens().faded.unwrap());
            assert!(text.contains("enter done"));
            let title_y = lines
                .iter()
                .position(|line| line.contains("draft prompt"))
                .unwrap();
            assert!(title_y > 8 && title_y < message - 4);
            assert!(
                !lines
                    .iter()
                    .any(|line| line.starts_with("draft prompt · Enter"))
            );
            insta::assert_snapshot!("dialog_panel", text);
        }
        match step {
            0 => {
                g.update(&Event::Paste(
                    "\nsecond\nthird\nfourth\nfifth\nsixth\nseventh".into(),
                ));
            }
            1 => {
                g.update(&key(KeyCode::F(3)));
            }
            2 => {
                g.update(&key(KeyCode::Esc));
            }
            3 => {
                g.update(&key(KeyCode::F(6)));
            }
            _ => {
                g.update(&key(KeyCode::F(8)));
            }
        }
    }
}

#[test]
fn dialog_message_label_is_darker_than_the_live_activity_hint() {
    use canonic_tui::ratatui::style::Color;
    for (cycles, faded, live) in [
        (1, Color::Indexed(240), Color::Indexed(245)),
        (2, Color::Rgb(94, 96, 100), Color::Rgb(133, 135, 140)),
    ] {
        let mut gallery = Gallery::new();
        gallery.open_scenario(Component::Dialog, "main").unwrap();
        for _ in 0..cycles {
            gallery.update(&key(KeyCode::F(4)));
        }
        let mut terminal = Terminal::new(TestBackend::new(100, 40)).unwrap();
        terminal.draw(|f| gallery.render(f)).unwrap();
        let text = super::components::dump(terminal.backend().buffer());
        let message_y = u16::try_from(
            text.lines()
                .position(|l| l.starts_with("message "))
                .unwrap(),
        )
        .unwrap();
        let buffer = terminal.backend().buffer();
        for x in 0..7 {
            assert_eq!(buffer[(x, message_y)].fg, faded);
        }
        assert_eq!(buffer[(0, message_y + 1)].fg, faded);
        assert_eq!(buffer[(10, message_y - 1)].fg, live);
        assert_ne!(faded, live);
    }
}

#[test]
fn dialog_fallbacks_show_one_field_scroll_hint_without_reversed_host_bars() {
    use canonic_tui::ratatui::style::{Color, Modifier};
    for cycles in [0, 3] {
        let mut gallery = Gallery::new();
        gallery.open_scenario(Component::Dialog, "main").unwrap();
        for _ in 0..cycles {
            gallery.update(&key(KeyCode::F(4)));
        }
        for _ in 0..9 {
            gallery.update(&key(KeyCode::F(2)));
        }
        let mut terminal = Terminal::new(TestBackend::new(100, 40)).unwrap();
        terminal.draw(|f| gallery.render(f)).unwrap();
        let buffer = terminal.backend().buffer();
        let text = super::components::dump(buffer);
        assert_eq!(text.matches("more").count(), 1);
        let hint_y = u16::try_from(text.lines().position(|l| l.contains("more")).unwrap()).unwrap();
        assert!(
            text.lines()
                .nth(usize::from(hint_y))
                .unwrap()
                .contains("name ")
        );
        if cycles == 3 {
            for x in 0..100 {
                for y in [0, 38] {
                    assert!(!buffer[(x, y)].modifier.contains(Modifier::REVERSED));
                }
            }
            assert!(!buffer[(0, 3)].modifier.contains(Modifier::DIM));
        } else {
            let x = (0..100)
                .find(|&x| buffer[(x, hint_y)].symbol() == "↑")
                .unwrap();
            assert_eq!(buffer[(x, hint_y)].fg, Color::Reset);
            assert!(buffer[(x, hint_y)].modifier.contains(Modifier::DIM));
        }
    }
}

#[cfg(unix)]
const GALLERY_PTY_SCRIPT: &str = r"
import fcntl,os,pty,re,select,signal,struct,subprocess,sys,termios,time
master,slave=pty.openpty()
fcntl.ioctl(slave,termios.TIOCSWINSZ,struct.pack('HHHH',28,100,0,0))
before=termios.tcgetattr(slave)
child=subprocess.Popen([sys.argv[1],'--colors','truecolor'],env={**os.environ,'NO_COLOR':'1'},stdin=slave,stdout=slave,stderr=slave,start_new_session=True)
output=b''
def screen():
    cells=[[' ']*100 for _ in range(28)]
    row=column=0
    for match in re.finditer(r'\x1b\[[0-?]*[ -/]*[@-~]|.',output.decode(errors='replace'),re.S):
        token=match.group()
        if token.startswith('\x1b['):
            args=token[2:-1];command=token[-1]
            if args.startswith(('?', '>', '<')):continue
            values=[int(v or 0) for v in args.split(';')]
            if command in 'Hf':row=(values[0] or 1)-1;column=(values[1] if len(values)>1 else 1)-1
            elif command=='G':column=(values[0] or 1)-1
            elif command=='C':column+=values[0] or 1
            elif command=='D':column-=values[0] or 1
            elif command=='J' and values[0]==2:cells=[[' ']*100 for _ in range(28)]
            elif command=='K' and 0<=row<28:
                start=0 if values[0] in (1,2) else max(0,column)
                end=min(100,column+1) if values[0]==1 else 100
                for x in range(start,end):cells[row][x]=' '
            continue
        if token=='\r':column=0;continue
        if token=='\n':row+=1;continue
        if ord(token)<32:continue
        if 0<=row<28 and 0<=column<100:cells[row][column]=token
        column+=1
    return '\n'.join(''.join(line) for line in cells).encode()
def wait_for(text):
    global output
    deadline=time.monotonic()+10
    while text not in screen():
        assert child.poll() is None,output[-2000:]
        assert time.monotonic()<deadline,(text,screen())
        if select.select([master],[],[],0.1)[0]:output+=os.read(master,65536)
try:
    wait_for(b'Choose a component')
    os.write(master,b'\r')
    wait_for('picker · scenarios'.encode())
    os.write(master,b'\r')
    wait_for(b'Continue without this server')
    os.write(master,b'\r')
    wait_for(b'Selected skip')
    os.write(master,b'\x1bOQ'*4)
    wait_for(b'switch branch')
    os.write(master,b'\x07\x1b[200~example29\x1b[201~')
    wait_for(b'updated 29 hours ago')
    os.write(master,b'\r')
    wait_for(b'Selected branch-29')
    os.write(master,b'\x1b')
    wait_for('picker · scenarios'.encode())
    os.write(master,b'\x1b')
    wait_for(b'Choose a component')
    os.write(master,b'\x1b[B\r')
    wait_for('chat · scenarios'.encode())
    os.write(master,b'\r')
    wait_for(b'Add the header to both digest calls.')
    os.write(master,b'hello\r')
    wait_for(b'Simulated response:')
    wait_for('✓ done · 34s'.encode())
    os.write(master,b'\x1b')
    wait_for('chat · scenarios'.encode())
    os.write(master,b'\x1b')
    wait_for(b'Choose a component')
    os.write(master,b'\x1b[B\r')
    wait_for('editor · scenarios'.encode())
    os.write(master,b'\r')
    wait_for(b'Independent second draft')
    os.write(master,b'\x07\x1b[200~PTY FIRST\r\nline two\x1b[201~')
    wait_for(b'PTY FIRST')
    wait_for(b'line two')
    os.write(master,b'\t\x07\x1b[200~PTY SECOND\x1b[201~')
    wait_for(b'PTY SECOND')
    os.write(master,b'\x1bOR');time.sleep(0.1)
    os.write(master,b'\x1b[200~MODAL ONLY\x1b[201~')
    wait_for(b'MODAL ONLY')
    os.write(master,b'\x1b');time.sleep(0.1)
    fcntl.ioctl(slave,termios.TIOCSWINSZ,struct.pack('HHHH',8,15,0,0))
    os.kill(child.pid,signal.SIGWINCH);time.sleep(0.1)
    os.write(master,b'\x03')
    # Drain redraw output so a full PTY buffer cannot block Ctrl-C processing.
    deadline=time.monotonic()+5
    while child.poll() is None:
        assert time.monotonic()<deadline,'gallery did not exit after Ctrl-C'
        if select.select([master],[],[],0.1)[0]:output+=os.read(master,65536)
    assert child.returncode==0
    assert re.search(rb'\x1b\[(?:\d+;)*38;2;86;194;201(?:;\d+)*m',output),'explicit RGB accent suppressed by NO_COLOR'
    # Exercise restored canonical input; macOS clears its transient PENDIN flag
    # when the next input is processed, before comparing every saved mode.
    os.write(master,b'restored\r')
    assert select.select([slave],[],[],1)[0],'ordinary line input not restored'
    assert os.read(slave,1024)==b'restored\n','canonical line handling not restored'
    assert termios.tcgetattr(slave)==before,'raw mode not restored'
    while select.select([master],[],[],0.1)[0]:
        chunk=os.read(master,65536)
        if not chunk:break
        output+=chunk
    assert b'\x1b[?1049l' in output,'alternate screen not restored'
    for profile in ['auto','none']:
        output=b''
        fcntl.ioctl(slave,termios.TIOCSWINSZ,struct.pack('HHHH',28,100,0,0))
        child=subprocess.Popen([sys.argv[1],'--colors',profile],env={**os.environ,'NO_COLOR':'1'},stdin=slave,stdout=slave,stderr=slave,start_new_session=True)
        wait_for(b'Choose a component')
        os.write(master,b'\x03')
        deadline=time.monotonic()+5
        while child.poll() is None:
            assert time.monotonic()<deadline,'no-color gallery did not exit'
            if select.select([master],[],[],0.1)[0]:output+=os.read(master,65536)
        assert child.returncode==0
        assert not re.search(rb'\x1b\[(?:\d+;)*(?:38|48);',output),'no-color profile emitted colors'
finally:
    if child.poll() is None:child.kill();child.wait()
    os.close(master);os.close(slave)
";

#[cfg(unix)]
#[test]
fn real_terminal_navigation_resize_and_ctrl_c_restore_terminal_state() {
    let output = std::process::Command::new("python3")
        .args(["-c", GALLERY_PTY_SCRIPT])
        .arg(assert_cmd::cargo::cargo_bin!("tui-gallery"))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}
