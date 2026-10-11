//! Verifies modal routing, generation invalidation, return focus and clamped rendering.

use canonic_tui::{
    dialog::{ClosePolicy, Dialog, Dispatch, Invalid, Reason, Size, Stack},
    editor::{Editor, EditorView, Outcome},
    event::{
        Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent,
        MouseEventKind,
    },
    ratatui::{Terminal, backend::TestBackend, layout::Rect},
    theme::{ColorMode, Presentation, Symbols},
};
use proptest::prelude::*;

fn key(code: KeyCode) -> Event {
    Event::Key(KeyEvent::new(code, KeyModifiers::NONE))
}
fn dialog(id: &str, targets: Vec<u8>) -> Dialog<Editor, u8> {
    let mut body = Editor::default();
    body.set_text(id);
    Dialog {
        id: id.into(),
        title: format!("{id} dialog"),
        targets,
        body,
        size: Size {
            width: 30,
            height: 5,
        },
        close: ClosePolicy::default(),
        view: canonic_tui::dialog::DialogView::default(),
    }
}
fn stack() -> Stack<Editor, u8> {
    Stack::new(vec![0, 1]).expect("unique fixture targets")
}

#[test]
fn arrival_waits_for_a_pause_keeps_order_and_guards_only_confirmation_keys() {
    use canonic_tui::dialog::Arrival;
    use std::time::Duration;
    let mut stack = Stack::new_timed(vec![0]).unwrap();
    let area = Rect::new(0, 0, 80, 24);
    stack.typed();
    assert!(matches!(
        stack.arrive(dialog("first", vec![1])).unwrap(),
        Arrival::Waiting
    ));
    stack.arrive(dialog("second", vec![2])).unwrap();
    assert_eq!(stack.waiting_len(), 2);
    assert!(stack.activity().contains("first dialog · ctrl+g opens it"));
    stack.tick(Duration::from_millis(999));
    assert!(stack.is_empty());
    stack.typed();
    stack.tick(Duration::from_millis(999));
    assert!(stack.is_empty());
    stack.tick(Duration::from_millis(1));
    let first = stack.ticket().unwrap().clone();
    assert_eq!(stack.waiting_len(), 1);
    for code in [KeyCode::Enter, KeyCode::Char('2')] {
        assert!(matches!(
            stack.dispatch(&key(code), area, |_, _, _| panic!("opening guard")),
            Dispatch::Consumed
        ));
    }
    stack.dispatch(&key(KeyCode::Char('y')), area, |body, _, event| {
        body.update(event) != Outcome::Unhandled
    });
    assert_eq!(stack.body_mut(&first).unwrap().text(), "firsty");
    stack.tick(Duration::from_millis(300));
    stack.close(&first, Reason::Submitted).unwrap();
    assert_eq!(stack.focused(), Some(&0));
    stack.tick(Duration::ZERO);
    let second = stack.ticket().unwrap().clone();
    assert_eq!(stack.body_mut(&second).unwrap().text(), "second");
    assert!(!stack.accepts(&first));
}

#[test]
fn attention_notices_locked_escape_and_app_close_never_reach_the_body() {
    use std::time::Duration;
    let mut stack = Stack::new_timed(vec![0]).unwrap();
    let area = Rect::new(0, 0, 80, 24);
    stack.set_terminal_focused(false);
    let parent = stack.push(dialog("parent", vec![1])).unwrap();
    let attention = stack.take_attention();
    assert_eq!(attention.len(), 1);
    assert_eq!(attention[0].ticket, parent);
    assert_eq!(attention[0].title, "! canonic · needs you");
    assert!(stack.take_attention().is_empty());
    stack.tick(Duration::from_secs(2));
    assert!(stack.activity().contains("waiting for you · 2s"));
    stack.set_terminal_focused(true);
    let mut locked = dialog("locked", vec![2]);
    locked.close.escape = false;
    let child = stack.push(locked).unwrap();
    stack.dispatch(&key(KeyCode::Esc), area, |_, _, _| false);
    assert_eq!(stack.activity(), "! answer to continue");
    let ctrl_c = Event::Key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL));
    stack.dispatch(&ctrl_c, area, |_, _, _| {
        panic!("app close belongs above the body")
    });
    assert!(stack.activity().contains("ctrl+c again closes it"));
    let repeated = Event::Key(KeyEvent::new_with_kind(
        KeyCode::Char('c'),
        KeyModifiers::CONTROL,
        KeyEventKind::Repeat,
    ));
    stack.dispatch(&repeated, area, |_, _, _| panic!("repeat cannot close"));
    assert_eq!(stack.len(), 2);
    assert!(matches!(
        stack.dispatch(&ctrl_c, area, |_, _, _| panic!("app close")),
        Dispatch::Dismissed(_)
    ));
    assert!(stack.accepts(&parent));
    assert!(!stack.accepts(&child));
    stack.remove(&parent, "answered elsewhere\x1b").unwrap();
    assert_eq!(stack.activity(), "dialog closed · answered elsewhere");
    stack.tick(Duration::from_secs(3));
    assert_eq!(stack.activity(), "ready");
}

#[test]
fn explicit_open_queue_bounds_replacement_and_error_updates_keep_tickets_scoped() {
    use canonic_tui::dialog::DialogView;
    let mut stack = Stack::new_timed(vec![0]).unwrap();
    let area = Rect::new(0, 0, 80, 24);
    stack.typed();
    for i in 0..16 {
        stack
            .arrive(dialog(&format!("queued-{i}"), vec![1]))
            .unwrap();
    }
    assert!(matches!(
        stack.arrive(dialog("overflow", vec![1])),
        Err(Invalid::QueueLimit)
    ));
    assert!(matches!(
        stack.arrive(dialog("queued-0", vec![1])),
        Err(Invalid::DuplicateId)
    ));
    let withdrawn = stack.withdraw_waiting("queued-15").unwrap();
    assert_eq!(withdrawn.body.text(), "queued-15");
    assert!(stack.withdraw_waiting("missing").is_none());
    stack.arrive(withdrawn).unwrap();
    let ctrl_g = Event::Key(KeyEvent::new(KeyCode::Char('g'), KeyModifiers::CONTROL));
    stack.dispatch(&ctrl_g, area, |_, _, _| {
        panic!("waiting opens above background")
    });
    let ticket = stack.ticket().unwrap().clone();
    assert!(stack.update_view(
        &ticket,
        DialogView {
            message: "host failure".into(),
            message_error: true,
            footer: "enter retry · esc cancel".into(),
            ..DialogView::default()
        }
    ));
    assert_eq!(stack.body_mut(&ticket).unwrap().text(), "queued-0");
    let (replacement, _) = stack.replace(dialog("replacement", vec![2])).unwrap();
    assert!(stack.activity().contains("replaced queued-0 dialog"));
    assert!(!stack.update_view(&ticket, DialogView::default()));
    let closed = stack.clear();
    assert_eq!(closed.len(), 16);
    assert_eq!(closed[0].id, "replacement");
    assert_eq!(closed[1].id, "queued-1");
    assert_eq!(stack.waiting_len(), 0);
    assert!(!stack.accepts(&replacement));
    assert_eq!(stack.focused(), Some(&0));
}

#[test]
fn irreversible_and_typed_confirmation_require_deliberate_current_actions() {
    use canonic_tui::dialog::{Caution, Confirmation, Decision};
    use std::time::Duration;
    let mut confirm = Confirmation::default();
    let enter = key(KeyCode::Enter);
    assert_eq!(
        confirm.update(
            &key(KeyCode::Char('2')),
            "delete",
            &Caution::Irreversible,
            ""
        ),
        Decision::None
    );
    assert_eq!(
        confirm.update(
            &Event::Paste("delete".into()),
            "delete",
            &Caution::Irreversible,
            ""
        ),
        Decision::None
    );
    assert_eq!(
        confirm.update(&enter, "delete-v1", &Caution::Irreversible, ""),
        Decision::Armed
    );
    let repeat = Event::Key(KeyEvent::new_with_kind(
        KeyCode::Enter,
        KeyModifiers::NONE,
        KeyEventKind::Repeat,
    ));
    assert_eq!(
        confirm.update(&repeat, "delete-v1", &Caution::Irreversible, ""),
        Decision::None
    );
    assert_eq!(
        confirm.update(&enter, "delete-v2", &Caution::Irreversible, ""),
        Decision::Armed
    );
    confirm.tick(Duration::from_secs(3));
    assert_eq!(
        confirm.update(&enter, "delete-v2", &Caution::Irreversible, ""),
        Decision::Armed
    );
    confirm.tick(Duration::from_secs(2));
    assert_eq!(
        confirm.update(&enter, "delete-v2", &Caution::Irreversible, ""),
        Decision::Confirm
    );
    let caution = Caution::TypeName("acme#22".into());
    assert_eq!(
        confirm.update(&enter, "delete", &caution, " acme#22"),
        Decision::NameMismatch
    );
    assert_eq!(
        confirm.update(&enter, "delete", &caution, "ACME#22"),
        Decision::NameMismatch
    );
    assert_eq!(
        confirm.update(&enter, "delete", &caution, "acme#22"),
        Decision::Confirm
    );
}

#[test]
fn nested_routing_restores_surviving_focus_and_parent_draft() {
    let mut stack = stack();
    stack.advance(false);
    assert_eq!(stack.focused(), Some(&1));
    let parent = stack.push(dialog("parent", vec![2, 3])).unwrap();
    stack.advance(false);
    let child = stack.push(dialog("child", vec![4, 5])).unwrap();
    let area = Rect::new(0, 0, 80, 24);
    assert!(matches!(
        stack.dispatch(
            &Event::Paste(" ONLY CHILD".into()),
            area,
            |body, focus, event| {
                assert_eq!(focus, Some(&4));
                body.update(event) != Outcome::Unhandled
            }
        ),
        Dispatch::Consumed
    ));
    assert!(stack.body_mut(&parent).is_none());
    assert_eq!(stack.body_mut(&child).unwrap().text(), "child ONLY CHILD");
    stack.dispatch(&key(KeyCode::Tab), area, |_, _, _| {
        panic!("Tab is scope navigation")
    });
    assert_eq!(stack.focused(), Some(&5));
    stack.remove_target(&3);
    stack.remove_target(&1);
    let Dispatch::Dismissed(closed) = stack.dispatch(&key(KeyCode::Esc), area, |body, _, event| {
        body.update(event) != Outcome::Unhandled
    }) else {
        panic!("Escape closes top");
    };
    assert_eq!(closed.reason, Reason::Cancelled);
    assert_eq!(closed.body.text(), "child ONLY CHILD");
    assert_eq!(stack.focused(), Some(&2));
    assert_eq!(stack.body_mut(&parent).unwrap().text(), "parent");
    assert!(stack.close(&child, Reason::Submitted).is_none());
    stack.close(&parent, Reason::Cancelled).unwrap();
    assert_eq!(stack.focused(), Some(&0));
}

#[test]
fn selection_consumes_escape_before_one_layer_dismisses_and_releases_are_inert() {
    let mut stack = stack();
    let ticket = stack.push(dialog("draft", vec![2])).unwrap();
    stack
        .body_mut(&ticket)
        .unwrap()
        .update(&Event::Key(KeyEvent::new(
            KeyCode::Char('g'),
            KeyModifiers::CONTROL,
        )));
    let area = Rect::new(0, 0, 80, 24);
    let result = stack.dispatch(&key(KeyCode::Esc), area, |body, _, event| {
        body.update(event) != Outcome::Unhandled
    });
    assert!(matches!(result, Dispatch::Consumed));
    assert_eq!(stack.len(), 1);
    let release = Event::Key(KeyEvent::new_with_kind(
        KeyCode::Esc,
        KeyModifiers::NONE,
        KeyEventKind::Release,
    ));
    stack.dispatch(&release, area, |_, _, _| {
        panic!("release cannot enter body")
    });
    assert_eq!(stack.len(), 1);
    assert!(matches!(
        stack.dispatch(&key(KeyCode::Esc), area, |body, _, event| body
            .update(event)
            != Outcome::Unhandled),
        Dispatch::Dismissed(_)
    ));
    assert!(matches!(
        stack.dispatch(&key(KeyCode::Enter), area, |_, _, _| panic!("no body")),
        Dispatch::Background
    ));
}

#[test]
fn replacement_clear_and_foreign_generations_cannot_mutate_a_new_opening() {
    let mut stack = stack();
    let first = stack.push(dialog("same", vec![2])).unwrap();
    let mut foreign = Stack::new(vec![0]).unwrap();
    let other = foreign.push(dialog("same", vec![2])).unwrap();
    assert!(!stack.accepts(&other));
    let (second, old) = stack.replace(dialog("same", vec![3])).unwrap();
    assert_eq!(old.ticket, first);
    assert_eq!(old.reason, Reason::Replaced);
    assert!(stack.body_mut(&first).is_none());
    assert!(stack.close(&first, Reason::Submitted).is_none());
    let child = stack.push(dialog("child", vec![4])).unwrap();
    assert!(!stack.accepts(&second));
    let closed = stack.clear();
    assert_eq!(closed.len(), 2);
    assert_eq!(closed[0].ticket, child);
    assert_eq!(closed[1].ticket, second);
    assert!(closed.iter().all(|d| d.reason == Reason::Cleared));
    assert_eq!(stack.focused(), Some(&0));
    let third = stack.push(dialog("same", vec![2])).unwrap();
    assert_ne!(third, second);
    assert!(!stack.accepts(&second));
}

#[test]
fn invalid_push_and_replace_leave_stack_focus_and_body_intact() {
    assert!(matches!(
        Stack::<Editor, u8>::new(vec![0, 0]),
        Err(Invalid::DuplicateTarget)
    ));
    let mut stack = stack();
    assert!(matches!(
        stack.replace(dialog("none", vec![])),
        Err(Invalid::Empty)
    ));
    let first = stack.push(dialog("first", vec![2])).unwrap();
    assert_eq!(
        stack.push(dialog("first", vec![3])).unwrap_err(),
        Invalid::DuplicateId
    );
    assert_eq!(
        stack.push(dialog("next", vec![3, 3])).unwrap_err(),
        Invalid::DuplicateTarget
    );
    let second = stack.push(dialog("second", vec![4])).unwrap();
    assert!(matches!(
        stack.replace(dialog("first", vec![5])),
        Err(Invalid::DuplicateId)
    ));
    assert!(stack.accepts(&second));
    assert_eq!(stack.focused(), Some(&4));
    for i in 2..16 {
        stack.push(dialog(&format!("layer-{i}"), vec![])).unwrap();
    }
    assert_eq!(
        stack.push(dialog("overflow", vec![])).unwrap_err(),
        Invalid::DepthLimit
    );
    assert_eq!(stack.len(), 16);
    stack.clear();
    assert!(!stack.accepts(&first));
}

#[test]
fn outside_click_is_opt_in_and_never_reaches_background_or_body() {
    let area = Rect::new(5, 4, 60, 20);
    let mouse = Event::Mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 5,
        row: 4,
        modifiers: KeyModifiers::NONE,
    });
    let mut stack = stack();
    let ticket = stack.push(dialog("locked", vec![2])).unwrap();
    assert!(matches!(
        stack.dispatch(&mouse, area, |_, _, _| panic!("outside swallowed")),
        Dispatch::Consumed
    ));
    assert!(stack.accepts(&ticket));
    let release = Event::Mouse(MouseEvent {
        kind: MouseEventKind::Up(MouseButton::Left),
        column: 5,
        row: 4,
        modifiers: KeyModifiers::NONE,
    });
    assert!(matches!(
        stack.dispatch(&release, area, |_, _, _| panic!(
            "outside releases never enter a body"
        )),
        Dispatch::Consumed
    ));
    let mut next = dialog("opt-in", vec![3]);
    next.close.outside_click = true;
    stack.push(next).unwrap();
    let Dispatch::Dismissed(closed) =
        stack.dispatch(&mouse, area, |_, _, _| panic!("outside never forwarded"))
    else {
        panic!("opt-in dismissal");
    };
    assert_eq!(closed.reason, Reason::Outside);
    assert!(stack.accepts(&ticket));
    let mut locked = dialog("no-escape", vec![3]);
    locked.close.escape = false;
    stack.replace(locked).unwrap();
    assert!(matches!(
        stack.dispatch(&key(KeyCode::Esc), area, |_, _, _| false),
        Dispatch::Consumed
    ));
    assert_eq!(stack.len(), 1);
}

#[test]
fn rendering_uses_shared_rules_and_focus_only_for_top_even_at_tiny_sizes() {
    let mut stack = stack();
    stack.push(dialog("parent", vec![2])).unwrap();
    stack.push(dialog("child", vec![3])).unwrap();
    let p = Presentation {
        colors: ColorMode::NoColor,
        symbols: Symbols::Ascii,
        ..Presentation::default()
    };
    let mut terminal = Terminal::new(TestBackend::new(30, 9)).unwrap();
    let mut targets = Vec::new();
    terminal
        .draw(|frame| {
            stack.render(frame, frame.area(), p, |body, frame, area, target| {
                targets.push(target.copied());
                body.render_with(
                    frame,
                    area,
                    EditorView {
                        chrome: false,
                        presentation: p,
                        ..EditorView::default()
                    },
                    target.is_some(),
                );
            });
        })
        .unwrap();
    assert_eq!(targets, vec![Some(3)]);
    assert_eq!(
        stack.area(Rect::new(0, 0, 30, 9)),
        Some(Rect::new(2, 2, 26, 5))
    );
    let text = super::components::dump(terminal.backend().buffer());
    assert!(text.contains("child dialog"));
    assert!(text.contains("esc cancel"));
    insta::assert_snapshot!("dialog_stack", text);
    for (width, height) in [(0, 0), (1, 1), (2, 2), (4, 3), (80, 30)] {
        let area = Rect::new(0, 0, width, height);
        let rect = stack.area(area).unwrap();
        assert!(rect.width <= width && rect.height <= height);
        let mut terminal = Terminal::new(TestBackend::new(width.max(1), height.max(1))).unwrap();
        let mut body_area = None;
        terminal
            .draw(|frame| {
                stack.render(frame, area, p, |body, frame, area, target| {
                    body_area = Some(area);
                    body.render_with(
                        frame,
                        area,
                        EditorView {
                            chrome: false,
                            presentation: p,
                            ..EditorView::default()
                        },
                        target.is_some(),
                    );
                });
            })
            .unwrap();
        if height < 3 && width > 0 {
            assert_eq!(body_area, Some(rect));
        }
    }
}

proptest! {
    #[test]
    fn arbitrary_stack_changes_keep_active_tickets_unique(operations in prop::collection::vec(0_u8..4,0..100)) {
        let mut stack=stack();let mut retired=Vec::new();let mut next=0;
        for operation in operations {
            match operation {
                0=>{let _=stack.push(dialog(&format!("id-{next}"),vec![2]));next+=1;}
                1=>{if let Some(ticket)=stack.ticket().cloned(){stack.close(&ticket,Reason::Cancelled);retired.push(ticket);}}
                2=>{if !stack.is_empty(){let (_,old)=stack.replace(dialog(&format!("id-{next}"),vec![2])).unwrap();next+=1;retired.push(old.ticket);}}
                _=>retired.extend(stack.clear().into_iter().map(|d|d.ticket)),
            }
            prop_assert!(stack.len()<=16);
            prop_assert!(retired.iter().all(|ticket|!stack.accepts(ticket)));
        }
    }
}
