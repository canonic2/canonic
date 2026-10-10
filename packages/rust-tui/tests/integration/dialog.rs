//! Verifies modal routing, generation invalidation, return focus and clamped rendering.

use canonic_rust_tui::{
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
    }
}
fn stack() -> Stack<Editor, u8> {
    Stack::new(vec![0, 1]).expect("unique fixture targets")
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
    assert_eq!(targets, vec![None, Some(3)]);
    assert_eq!(
        stack.area(Rect::new(0, 0, 30, 9)),
        Some(Rect::new(0, 2, 30, 5))
    );
    let text = super::components::dump(terminal.backend().buffer());
    assert!(text.contains("child dialog"));
    assert!(text.contains("esc close"));
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
