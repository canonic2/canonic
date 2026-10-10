//! Exercises measured centered panels, fallback boxes, fading and caret layout.

use canonic_rust_tui::{
    dialog::{ClosePolicy, Dialog, DialogView, Size, Stack},
    editor::{Editor, EditorView},
    event::{Event, KeyCode, KeyEvent, KeyModifiers},
    ratatui::{
        Terminal,
        backend::TestBackend,
        layout::Rect,
        style::{Color, Modifier},
        widgets::{Block, Paragraph},
    },
    theme::{ColorMode, Presentation},
};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

fn layer() -> Dialog<Editor, u8> {
    let mut editor = Editor::default();
    editor.set_text("Café · 界 · 👩‍💻");
    Dialog {
        id: "rename".into(),
        title: "rename branch".into(),
        targets: vec![1],
        body: editor,
        size: Size {
            width: 100,
            height: 14,
        },
        close: ClosePolicy::default(),
        view: DialogView {
            scope: "example project".into(),
            message: "Renames the local example branch.".into(),
            footer: "enter rename · esc cancel".into(),
            compact_footer: "enter · esc".into(),
        },
    }
}
fn render(
    stack: &mut Stack<Editor, u8>,
    terminal: &mut Terminal<TestBackend>,
    viewport: Rect,
    p: Presentation,
) -> Rect {
    terminal
        .draw(|f| {
            f.render_widget(
                Paragraph::new("visible background draft").style(p.base().fg(Color::White)),
                f.area(),
            );
            stack.render_with(
                f,
                viewport,
                p,
                |e, width, boxed| {
                    u16::try_from(
                        e.visual_rows(width, p.symbols).clamp(1, 6) + usize::from(boxed) * 2,
                    )
                    .expect("field height is bounded by eight rows")
                },
                |e, f, area, target, boxed| {
                    f.render_widget(
                        Block::default().style(p.base().fg(Color::Reset).bg(Color::Reset)),
                        area,
                    );
                    e.render_with(
                        f,
                        area,
                        EditorView {
                            title: "name",
                            chrome: boxed && area.height >= 3,
                            presentation: p,
                            ..EditorView::default()
                        },
                        target.is_some(),
                    );
                    None
                },
            );
        })
        .unwrap();
    stack.area(viewport).expect("fixture has an open dialog")
}

#[test]
fn panel_grows_and_recenters_with_full_grapheme_caret() {
    let mut stack = Stack::new(vec![0]).unwrap();
    let ticket = stack.push(layer()).unwrap();
    let mut terminal = Terminal::new(TestBackend::new(100, 40)).unwrap();
    let viewport = Rect::new(5, 3, 80, 24);
    let p = Presentation {
        colors: ColorMode::TrueColor,
        ..Presentation::default()
    };
    let area = render(&mut stack, &mut terminal, viewport, p);
    assert_eq!(area, Rect::new(9, 13, 72, 4));
    let buffer = terminal.backend().buffer();
    assert_eq!(buffer[(9, 13)].bg, p.tokens().surface.unwrap());
    assert_eq!(buffer[(9, 14)].bg, p.tokens().surface.unwrap());
    assert_eq!(buffer[(10, 15)].bg, Color::Reset);
    assert_eq!(buffer[(9, 15)].bg, p.tokens().surface.unwrap());
    assert_eq!(buffer[(80, 15)].bg, p.tokens().surface.unwrap());
    assert_eq!(buffer[(0, 0)].fg, Color::Rgb(94, 96, 100));
    assert_eq!(
        terminal.get_cursor_position().unwrap(),
        (10 + u16::try_from("Café · 界 · 👩‍💻".width()).unwrap(), 15).into()
    );
    let after = terminal.get_cursor_position().unwrap();
    stack
        .body_mut(&ticket)
        .unwrap()
        .update(&Event::Key(KeyEvent::new(
            KeyCode::Left,
            KeyModifiers::NONE,
        )));
    render(&mut stack, &mut terminal, viewport, p);
    let before = terminal.get_cursor_position().unwrap();
    assert_eq!(before.x + 2, after.x);
    assert_eq!(before.y, after.y);
    stack
        .body_mut(&ticket)
        .unwrap()
        .update(&Event::Key(KeyEvent::new(
            KeyCode::Right,
            KeyModifiers::NONE,
        )));
    render(&mut stack, &mut terminal, viewport, p);
    assert_eq!(terminal.get_cursor_position().unwrap(), after);
    stack
        .body_mut(&ticket)
        .unwrap()
        .set_text("one\ntwo\nthree\nfour\nfive");
    let grown = render(&mut stack, &mut terminal, viewport, p);
    assert_eq!(grown.height, 8);
    assert_eq!(grown.y + grown.height / 2, area.y + area.height / 2);
    let panel = render(&mut stack, &mut terminal, viewport, p);
    assert_eq!(panel, Rect::new(9, 11, 72, 8));
    assert!(stack.accepts(&ticket));
    assert_eq!(
        stack.body_mut(&ticket).unwrap().text(),
        "one\ntwo\nthree\nfour\nfive"
    );
}

#[test]
fn fallback_boxes_and_narrow_chrome_keep_the_active_body_reachable() {
    for colors in [
        ColorMode::Ansi16,
        ColorMode::Ansi256,
        ColorMode::TrueColor,
        ColorMode::NoColor,
    ] {
        let mut stack = Stack::new(vec![0]).unwrap();
        stack.push(layer()).unwrap();
        let mut terminal = Terminal::new(TestBackend::new(40, 40)).unwrap();
        let p = Presentation {
            colors,
            ..Presentation::default()
        };
        let area = render(&mut stack, &mut terminal, Rect::new(0, 0, 40, 30), p);
        let text = super::components::dump(terminal.backend().buffer());
        assert!(!text.contains("example project"));
        assert!(text.contains("enter · esc"));
        let is_boxed = matches!(colors, ColorMode::Ansi16 | ColorMode::NoColor);
        let mut x = area.x + 1;
        let y = area.y + 2 + u16::from(is_boxed);
        for grapheme in "Café · 界 · 👩‍💻".graphemes(true) {
            assert_eq!(terminal.backend().buffer()[(x, y)].symbol(), grapheme);
            x += u16::try_from(grapheme.width()).unwrap();
        }
        assert_eq!(
            terminal.backend().buffer()[(area.x, area.y)].symbol() == "┌",
            is_boxed
        );
        assert_eq!(
            terminal.backend().buffer()[(area.x + 1, area.y + 2)].symbol(),
            if is_boxed { "n" } else { "C" }
        );
        assert!(
            terminal.backend().buffer()[(area.x + 1 + u16::from(is_boxed), area.y)]
                .modifier
                .contains(Modifier::BOLD)
        );
        if colors == ColorMode::NoColor {
            assert!(
                !terminal.backend().buffer()[(0, 0)]
                    .modifier
                    .contains(Modifier::DIM)
            );
            assert!(
                !terminal.backend().buffer()[(area.x + 2, area.y)]
                    .modifier
                    .contains(Modifier::REVERSED)
            );
            assert!(
                terminal.backend().buffer()[(area.x + 1, area.y + 2)]
                    .modifier
                    .contains(Modifier::REVERSED)
            );
        }
        if is_boxed {
            for y in [area.y, area.bottom() - 1] {
                assert_eq!(terminal.backend().buffer()[(area.x + 1, y)].symbol(), " ");
                assert_eq!(
                    terminal.backend().buffer()[(area.right() - 2, y)].symbol(),
                    " "
                );
            }
        }
        if colors == ColorMode::Ansi16 {
            let buffer = terminal.backend().buffer();
            assert_eq!(buffer[(area.x + 2, area.y)].fg, Color::Cyan);
            let hint = &buffer[(area.right() - 5, area.bottom() - 1)];
            assert_eq!(hint.fg, Color::Reset);
            assert!(hint.modifier.contains(Modifier::DIM));
        }
        for (width, height) in [(1, 1), (2, 2), (3, 3), (18, 8)] {
            terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
            let tiny = render(&mut stack, &mut terminal, Rect::new(0, 0, width, height), p);
            assert!(tiny.width <= width && tiny.height <= height);
            assert!(terminal.backend().cursor_visible());
        }
    }
}
