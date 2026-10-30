//! Local artifact chooser; confirmation produces an identified Chat intent only.

use crate::{
    chat::{Chat, Content},
    dialog::{ClosePolicy, Dialog, DialogView, Reason, Size, Stack},
    event::Event,
    picker::{Choice, ChoicePicker, Filter, Intent, SelectionMode},
    theme::Presentation,
};
use ratatui::{Frame, layout::Rect};

pub(super) struct Chooser {
    stack: Stack<ChoicePicker, u8>,
    message: String,
    part: String,
    call: String,
    viewport: Rect,
}
impl Chooser {
    pub(super) fn new(chat: &Chat, message: &str, part: &str, call: &str) -> Option<Self> {
        let view = chat.tools.get(&(message.into(), part.into()))?;
        let card = match view {
            super::tools::View::Single(card) if card.call().id == call => card.as_ref(),
            super::tools::View::Group(group) => group.card(call)?,
            super::tools::View::Single(_) => return None,
        };
        if !card
            .call()
            .artifacts
            .iter()
            .enumerate()
            .any(|(index, _)| card.available(crate::tool_call_card::Command::OpenArtifact(index)))
        {
            return None;
        }
        let part = chat.turn(message)?.parts.iter().find(|p| p.id == part)?;
        let call = match &part.content {
            Content::ToolCard(c) if c.id == call => c.as_ref(),
            Content::ToolGroup(group) => group.calls.iter().find(|c| c.id == call)?,
            _ => return None,
        };
        if !call.artifacts.iter().any(|a| a.open_available) {
            return None;
        }
        let choices = call
            .artifacts
            .iter()
            .map(|a| {
                let mut c = Choice::new(&a.id, &a.label);
                if !a.open_available {
                    c.disabled = Some("host does not offer opening".into());
                }
                c
            })
            .collect();
        let picker = ChoicePicker::new(choices, None, SelectionMode::Single, Filter::Off).ok()?;
        let mut stack = Stack::new_timed(vec![0]).ok()?;
        stack
            .push(Dialog {
                id: "artifacts".into(),
                title: "artifacts".into(),
                targets: vec![1],
                body: picker,
                size: Size::default(),
                close: ClosePolicy::default(),
                view: DialogView {
                    footer: "↑↓ choose · enter open · esc close".into(),
                    compact_footer: "enter · esc".into(),
                    ..DialogView::default()
                },
            })
            .ok()?;
        Some(Self {
            stack,
            message: message.into(),
            part: part.id.clone(),
            call: call.id.clone(),
            viewport: Rect::default(),
        })
    }
    pub(super) fn update(&mut self, chat: &mut Chat, event: &Event) -> bool {
        let Some(ticket) = self.stack.ticket().cloned() else {
            return false;
        };
        let mut result = Intent::None;
        self.stack
            .dispatch(event, self.viewport, |picker, _, event| {
                result = picker.update(event);
                result != Intent::Cancelled
            });
        if let Intent::Selected(id) = result
            && chat.open_tool_artifact(&self.message, &self.part, &self.call, &id)
        {
            self.stack.close(&ticket, Reason::Submitted);
        }
        !self.stack.is_empty()
    }
    pub(super) fn render(
        &mut self,
        frame: &mut Frame<'_>,
        area: Rect,
        activity: Rect,
        theme: Presentation,
    ) {
        self.viewport = area;
        self.stack.render_with(
            frame,
            area,
            theme,
            |_, _, _| 5,
            |p, f, a, _, _| {
                p.render_list(f, a, theme, true);
                None
            },
        );
        if self
            .stack
            .area(area)
            .is_none_or(|panel| !panel.intersects(activity))
        {
            frame.render_widget(ratatui::widgets::Clear, activity);
            frame.render_widget(
                ratatui::widgets::Paragraph::new(self.stack.activity()).style(
                    theme
                        .warning()
                        .remove_modifier(ratatui::style::Modifier::DIM),
                ),
                activity,
            );
        }
    }
    pub(super) fn tick(&mut self, elapsed: std::time::Duration) {
        self.stack.tick(elapsed);
    }
}
