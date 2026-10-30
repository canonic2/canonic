//! Composes rich picker fixtures with host-owned placement, focus and an editor draft.

use crate::{
    editor::{Editor, EditorView},
    event::{Event, KeyCode, KeyEventKind},
    focus::Focus,
    picker::{Choice, ChoicePicker, Filter, Intent, PickerView, SelectionMode, Status},
    theme::Presentation,
};
use ratatui::{
    Frame,
    layout::Rect,
    text::{Line, Span},
    widgets::Paragraph,
};

const NAMES: [&str; 16] = [
    "default", "details", "disabled", "danger", "filter", "no-match", "scrolled", "multi",
    "groups", "loading", "narrow", "done", "empty", "error", "maximum", "minimum",
];

pub(super) struct PickerDemo {
    picker: ChoicePicker,
    draft: Editor,
    focus: Focus<usize>,
    scenario: usize,
    host: usize,
    result: Option<String>,
    note: String,
}
impl PickerDemo {
    pub(super) fn new() -> Self {
        let mut draft = Editor::default();
        draft.set_text("Independent host draft");
        Self {
            picker: fixture(0),
            draft,
            focus: Focus::new(vec![0, 1])
                .unwrap_or_else(|| unreachable!("fixture IDs are distinct")),
            scenario: 0,
            host: 1,
            result: None,
            note: String::new(),
        }
    }
    pub(super) fn tick(&mut self, elapsed: std::time::Duration) {
        self.picker.tick(elapsed);
        self.draft.tick(elapsed);
    }
    pub(super) fn input(&self) -> &str {
        self.draft.text()
    }
    pub(super) fn update(&mut self, event: &Event) -> bool {
        if let Event::Key(key) = event {
            if key.kind == KeyEventKind::Release {
                return true;
            }
            match key.code {
                KeyCode::F(2) => {
                    self.scenario = (self.scenario + 1) % NAMES.len();
                    self.picker = fixture(self.scenario);
                    self.result = None;
                    self.note.clear();
                    self.focus = Focus::new(vec![0, 1])
                        .unwrap_or_else(|| unreachable!("fixture IDs are distinct"));
                    return true;
                }
                KeyCode::F(6) => {
                    self.host = (self.host + 1) % 3;
                    return true;
                }
                KeyCode::F(5) => {
                    self.picker.set_status(Status::Ready);
                    let id = self.picker.selected().map(str::to_owned);
                    let choices = self
                        .picker
                        .choices()
                        .iter()
                        .filter(|c| Some(&c.id) != id.as_ref())
                        .cloned()
                        .collect();
                    if self.picker.replace_choices(choices).is_err() {
                        unreachable!("filtering cannot duplicate fixture identities");
                    }
                    "Refreshed source · removed highlighted identity".clone_into(&mut self.note);
                    return true;
                }
                KeyCode::Tab | KeyCode::BackTab => {
                    self.focus.advance(
                        key.code == KeyCode::BackTab
                            || key.modifiers.contains(crate::event::KeyModifiers::SHIFT),
                    );
                    return true;
                }
                KeyCode::F(4) => return false,
                KeyCode::Char('c' | 't')
                    if key.modifiers.contains(crate::event::KeyModifiers::CONTROL) =>
                {
                    return false;
                }
                KeyCode::Esc if self.focus.active() == Some(&1) => {
                    return self.draft.update(event) != crate::editor::Outcome::Unhandled;
                }
                KeyCode::Esc if self.result.is_some() || self.scenario == 11 => return false,
                _ => {}
            }
        }
        if self.focus.active() == Some(&1) {
            self.draft.update(event);
            return true;
        }
        if self.result.is_some() || self.scenario == 11 {
            return true;
        }
        match self.picker.update(event) {
            Intent::Cancelled => {
                self.result = Some("→ cancelled".into());
                self.note = "Cancelled · no selection confirmed".into();
            }
            Intent::Selected(id) => {
                self.note = format!("Selected {id} · fixture only; no settings changed");
                self.result = Some(format!("github server -> {id}"));
            }
            Intent::Confirmed(ids) => {
                self.note = format!("Confirmed [{}] · fixture only", ids.join(", "));
                self.result = Some(format!("marked set -> [{}]", ids.join(", ")));
            }
            Intent::Inspect(id) => {
                if let Some(c) = self.picker.choices().iter().find(|c| c.id == id) {
                    self.note = format!(
                        "Inspect {}: {} · no selection intent",
                        c.label,
                        c.disabled.as_deref().unwrap_or(&c.detail)
                    );
                }
            }
            Intent::None => {}
        }
        true
    }
    pub(super) fn render(&mut self, frame: &mut Frame<'_>, area: Rect, p: Presentation) {
        let hints_y = area.bottom().saturating_sub(2).max(area.y);
        let dock_h = area.height.saturating_sub(2).min(5);
        let dock_y = hints_y.saturating_sub(dock_h).max(area.y);
        self.draft.render_with(
            frame,
            Rect::new(area.x, dock_y, area.width, dock_h),
            EditorView {
                title: "message",
                presentation: p,
                ..EditorView::default()
            },
            self.focus.active() == Some(&1),
        );
        let activity_y = dock_y.saturating_sub(1).max(area.y);
        if activity_y < dock_y {
            self.render_activity(frame, Rect::new(area.x, activity_y, area.width, 1), p);
        }
        let available = Rect::new(
            area.x,
            area.y,
            area.width,
            activity_y.saturating_sub(area.y),
        );
        let width = if self.scenario == 10 {
            area.width.min(40)
        } else {
            area.width
        };
        let branch = matches!(self.scenario, 4..=6);
        let view = PickerView {
            title: if branch {
                "switch branch"
            } else {
                "github server"
            },
            source: if self.scenario == 1 { ".mcp.json" } else { "" },
            context: if self.scenario == 1 {
                "Can read issues and pull requests with your token. The host owns this decision."
            } else {
                ""
            },
            max_height: if self.host == 2 { available.height } else { 9 },
            presentation: p,
            ..PickerView::default()
        };
        let y = if self.host == 1 {
            available
                .bottom()
                .saturating_sub(self.picker.height(width, view).min(available.height))
                .max(available.y)
        } else if self.host == 0 {
            available.y.saturating_add(2).min(available.bottom())
        } else {
            available.y
        };
        if self.host != 2 && available.height > 0 {
            frame.render_widget(
                Paragraph::new("Acme · host context / local fixture").style(p.muted()),
                Rect::new(area.x, area.y, area.width, 1),
            );
        }
        let picker_area = Rect::new(area.x, y, width, available.bottom().saturating_sub(y));
        if let Some(result) = self.result.as_deref().or(if self.scenario == 11 {
            Some("github server -> skip")
        } else {
            None
        }) {
            frame.render_widget(Paragraph::new(result).style(p.accent()), picker_area);
        } else {
            self.picker
                .render(frame, picker_area, view, self.focus.active() == Some(&0));
        }
        let hint = format!(
            "#picker:{} · F2 fixture · F3 inspect · F5 refresh · F6 {} · Tab focus",
            NAMES[self.scenario],
            ["stage", "dock", "full screen"][self.host]
        );
        frame.render_widget(
            Paragraph::new(hint).style(p.muted()),
            Rect::new(area.x, hints_y, area.width, 1.min(area.height)),
        );
        if area.height > 1 {
            frame.render_widget(
                Paragraph::new(self.note.as_str()).style(p.accent()),
                Rect::new(area.x, hints_y + 1, area.width, 1),
            );
        }
    }

    fn render_activity(&self, frame: &mut Frame<'_>, area: Rect, p: Presentation) {
        let status = if let Some(notice) = self.picker.notice() {
            notice
        } else if self.result.is_some() || self.scenario == 11 {
            "ready"
        } else if self.scenario == 9 {
            "reading supplied options"
        } else if self.scenario == 13 {
            "! source unavailable"
        } else if self.scenario == 12 {
            "ready"
        } else if matches!(self.scenario, 4..=7) {
            "! needs you · enter / esc"
        } else if self.scenario == 2 {
            "! needs you · 1 3"
        } else if p.symbols == crate::theme::Symbols::Ascii {
            "! needs you · 1-3"
        } else {
            "! needs you · 1–3"
        };
        let (label, detail) = status.split_once(" · ").unwrap_or((status, ""));
        let mut spans = vec![Span::styled(
            label,
            if label == "! needs you" {
                p.warning()
            } else {
                p.muted()
            },
        )];
        if !detail.is_empty() {
            spans.push(Span::styled(format!(" · {detail}"), p.muted()));
        }
        frame.render_widget(Paragraph::new(Line::from(spans)), area);
    }
}

fn servers() -> Vec<Choice> {
    [
        ("once", "Use this server", "only github"),
        ("all", "Use all project servers", "github and 2 others"),
        ("skip", "Continue without this server", "ask next session"),
    ]
    .into_iter()
    .map(|(id, label, detail)| {
        let mut row = Choice::new(id, label);
        detail.clone_into(&mut row.detail);
        row
    })
    .collect()
}
fn branches() -> Vec<Choice> {
    (0..31)
        .map(|i| {
            let mut c = Choice::new(
                format!("branch-{i}"),
                if i < 3 {
                    format!("fix/27{i}-example")
                } else {
                    format!("feature/example-{i}")
                },
            );
            c.detail = format!("updated {i} hours ago");
            c.current = i == 0;
            c
        })
        .collect()
}
fn fixture(scenario: usize) -> ChoicePicker {
    let mut choices = if matches!(scenario, 4..=6) {
        branches()
    } else {
        servers()
    };
    let mut filter = Filter::Auto;
    let mode = if matches!(scenario, 7 | 14 | 15) {
        filter = Filter::On;
        SelectionMode::Multiple
    } else {
        SelectionMode::Single
    };
    match scenario {
        2 => choices[1].disabled = Some("needs approval in org policy".into()),
        3 => {
            choices[2].label = "Remove from project configuration".into();
            choices[2].danger = true;
        }
        8 => {
            choices[0].group = "this project".into();
            choices[1].group = "this project".into();
            choices[2].group = "other".into();
        }
        12 => choices.clear(),
        _ => {}
    }
    let mut picker = ChoicePicker::new(
        choices,
        if scenario == 3 { None } else { Some("skip") },
        mode,
        filter,
    )
    .unwrap_or_else(|_| unreachable!("fixture IDs are unique"));
    picker.set_inspect_available(true);
    match scenario {
        4 => picker.set_query("fix 27"),
        5 => picker.set_query("releaze"),
        6 => {
            picker.update(&Event::Key(crate::event::KeyEvent::new(
                KeyCode::End,
                crate::event::KeyModifiers::NONE,
            )));
        }
        7 => picker.set_marked(&["once"]),
        9 => picker.set_status(Status::Loading("reading supplied options".into())),
        13 => picker.set_status(Status::Error(
            "source unavailable; F5 retries locally".into(),
        )),
        14 => {
            picker.set_selection_limits(0, Some(1));
            picker.set_marked(&["once"]);
        }
        15 => {
            picker.set_selection_limits(1, None);
        }
        _ => {}
    }
    picker
}
