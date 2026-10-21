//! Print ordering, exact acceptance and ambiguity boundaries without terminal ownership.
use canonic_tui::inline_composition::{Invalid, Part, PrintLedger};
fn part(id: &str, revision: u64, settled: bool) -> Part {
    Part {
        id: id.into(),
        revision,
        label: format!("card {id}"),
        text: format!("{id} revision {revision}"),
        settled,
    }
}
#[test]
fn settled_parts_wait_for_predecessors_and_updates_append_named_corrections() {
    let mut ledger = PrintLedger::new();
    ledger
        .replace(vec![
            part("a", 1, true),
            part("b", 1, false),
            part("c", 1, true),
        ])
        .unwrap();
    let first = ledger.ready().unwrap();
    assert_eq!(first.lines, ["a revision 1"]);
    assert!(ledger.ready().is_none());
    assert!(ledger.acknowledge(&first));
    assert_eq!(
        ledger
            .live()
            .iter()
            .map(|p| p.id.as_str())
            .collect::<Vec<_>>(),
        ["b", "c"]
    );
    ledger
        .replace(vec![
            part("a", 1, true),
            part("b", 2, true),
            part("c", 1, true),
        ])
        .unwrap();
    let next = ledger.ready().unwrap();
    assert_eq!(next.lines, ["b revision 2", "c revision 1"]);
    assert!(ledger.acknowledge(&next));
    ledger
        .replace(vec![
            part("a", 1, true),
            part("b", 3, true),
            part("c", 1, true),
        ])
        .unwrap();
    let correction = ledger.ready().unwrap();
    assert_eq!(correction.lines, ["↳ card b → b revision 3"]);
    assert!(ledger.acknowledge(&correction));
    assert!(ledger.ready().is_none());
}
#[test]
fn pending_versions_are_frozen_and_receipts_are_exact_and_single_use() {
    let mut ledger = PrintLedger::new();
    ledger.replace(vec![part("a", 1, true)]).unwrap();
    let first = ledger.ready().unwrap();
    ledger.replace(vec![part("a", 2, true)]).unwrap();
    let mut altered = first.clone();
    altered.lines[0] = "wrong output".into();
    assert!(!ledger.acknowledge(&altered));
    let mut foreign = PrintLedger::new();
    foreign.replace(vec![part("a", 1, true)]).unwrap();
    assert!(!ledger.acknowledge(&foreign.ready().unwrap()));
    assert!(ledger.acknowledge(&first));
    assert!(!ledger.acknowledge(&first));
    assert_eq!(ledger.ready().unwrap().lines, ["↳ card a → a revision 2"]);
}
#[test]
fn invalid_replacement_is_atomic_and_cannot_forget_an_outstanding_revision() {
    let mut ledger = PrintLedger::new();
    ledger.replace(vec![part("a", 2, true)]).unwrap();
    let batch = ledger.ready().unwrap();
    ledger.replace(vec![]).unwrap();
    assert_eq!(
        ledger.replace(vec![part("a", 1, true)]),
        Err(Invalid::Revision)
    );
    assert_eq!(
        ledger.replace(vec![part("b", 1, true), part("b", 1, true)]),
        Err(Invalid::Identity)
    );
    let mut big = part("b", 1, true);
    big.text = "x".repeat(61 * 1024);
    assert_eq!(ledger.replace(vec![big]), Err(Invalid::Limit));
    assert!(ledger.acknowledge(&batch));
    assert_eq!(ledger.live(), [] as [&Part; 0]);
    assert_eq!(
        ledger.replace(vec![part("a", 1, true)]),
        Err(Invalid::Revision)
    );
}
#[test]
fn ambiguous_partial_write_stops_automatic_replay() {
    let mut ledger = PrintLedger::new();
    ledger.replace(vec![part("a", 1, true)]).unwrap();
    let batch = ledger.ready().unwrap();
    let mut altered = batch.clone();
    altered.lines.clear();
    assert!(!ledger.abort(&altered));
    assert!(ledger.abort(&batch));
    assert!(ledger.stopped());
    assert!(!ledger.acknowledge(&batch));
    ledger.replace(vec![part("a", 2, true)]).unwrap();
    assert!(ledger.ready().is_none());
}

#[test]
fn measured_chat_dock_retains_draft_and_fits_an_active_review() {
    use canonic_tui::{
        chat::Chat,
        question_form::{Kind, Question, QuestionForm, Request},
        ratatui::{Terminal, backend::TestBackend},
        review_queue::Review,
        theme::Presentation,
    };
    let mut chat = Chat::new();
    chat.set_input("retained draft");
    let p = Presentation::default();
    let dock = chat.inline_height(80, p);
    let form = QuestionForm::new(Request {
        id: "height-review".into(),
        revision: 1,
        title: "Measured review".into(),
        questions: vec![Question::new(
            "note",
            "Note",
            "Supply the final deployment note",
            Kind::Text,
        )],
        partial: false,
        back: false,
    })
    .unwrap();
    chat.push_review(Review::Questions(Box::new(form))).unwrap();
    let height = chat.inline_height(80, p);
    assert!(height > dock);
    assert!(chat.inline_height(24, p) >= height);
    let mut t = Terminal::new(TestBackend::new(80, height)).unwrap();
    t.draw(|f| chat.render_inline_live(f, f.area(), p)).unwrap();
    let text = super::components::dump(t.backend().buffer());
    assert!(text.contains("Measured review"), "{text}");
    assert!(text.contains("Supply the final deployment note"), "{text}");
    assert!(text.contains("retained draft"), "{text}");
    assert_eq!(chat.input(), "retained draft");
}

#[cfg(unix)]
#[test]
fn height_changes_preserve_output_and_defer_borrowed_screen_updates() {
    if std::env::var_os("CANONIC_TUI_INLINE_HEIGHT_CHILD").is_some() {
        height_child().expect("owned inline height probe");
        return;
    }
    let output = std::process::Command::new("python3")
        .args([
            "-B",
            concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/fixtures/inline_height.py"
            ),
        ])
        .arg(std::env::current_exe().unwrap())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}
#[cfg(unix)]
fn height_child() -> std::io::Result<()> {
    use canonic_tui::{
        event::{Event, KeyCode},
        ratatui::{layout::Rect, widgets::Paragraph},
        runtime::{Capabilities, InlineOptions, InlineSession},
    };
    println!("\nBEFORE SENTINEL");
    let mut session = InlineSession::open(InlineOptions {
        height: 4,
        capabilities: Capabilities {
            cursor: true,
            alternate: true,
        },
    })?;
    let mut stage = "short";
    loop {
        session.draw(|f| {
            let area = f.area();
            f.render_widget(
                Paragraph::new(format!("LIVE {stage}")),
                Rect { height: 1, ..area },
            );
            f.render_widget(
                Paragraph::new(format!("FOOT {stage}")),
                Rect::new(area.x, area.bottom().saturating_sub(1), area.width, 1),
            );
        })?;
        if let Some(Event::Key(k)) = session.poll(std::time::Duration::from_millis(50))? {
            match k.code {
                KeyCode::Char('g') => {
                    session.set_height(u16::MAX)?;
                    stage = "grown";
                }
                KeyCode::Char('s') => {
                    session.set_height(4)?;
                    stage = "shrunk";
                }
                KeyCode::Char('z') => {
                    let old = session.area();
                    assert_eq!(
                        session
                            .set_height(0)
                            .expect_err("zero height must fail")
                            .kind(),
                        std::io::ErrorKind::InvalidInput
                    );
                    assert_eq!(session.area(), old);
                    stage = "rejected";
                }
                KeyCode::Char('a') => {
                    session.enter_alternate()?;
                    stage = "alternate";
                }
                KeyCode::Char('d') => {
                    session.set_height(9)?;
                    stage = "deferred";
                }
                KeyCode::Char('r') => {
                    session.leave_alternate()?;
                    stage = "returned";
                }
                KeyCode::Char('p') => {
                    session.print_before("LATE SENTINEL")?;
                    stage = "late";
                }
                KeyCode::Char('f') => {
                    let paused = session.pause()?;
                    assert!(!crossterm::terminal::is_raw_mode_enabled()?);
                    println!("FOREGROUND SENTINEL");
                    session = paused.resume()?;
                    stage = "resumed";
                }
                KeyCode::Char('x') => {
                    session.finish("AFTER SENTINEL")?;
                    return Ok(());
                }
                _ => {}
            }
        }
    }
}
