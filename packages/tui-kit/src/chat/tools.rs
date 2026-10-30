//! Adapts public card state and identified effects into Chat's anchored blocks.

use super::{
    Chat, Content, Intent,
    content::{Block, Ink, Run},
    layout::{BlockId, Row},
    model::Anchor,
};
use crate::{
    event::{Event, KeyCode, KeyEventKind, KeyModifiers},
    theme::Symbols,
    tool_call_card::{Command, Ticket, ToolCallCard, ToolGroupCard},
};
use std::time::Duration;

pub(super) enum View {
    Single(Box<ToolCallCard>),
    Group(ToolGroupCard),
}
impl View {
    pub(super) fn anchor_blocks(&self, blocks: &mut [Block]) {
        if let Self::Group(group) = self {
            for block in blocks.iter_mut().skip(1) {
                if let super::content::Kind::Tool { id } = &block.kind {
                    block.start = group.slot(id) * 2_097_152;
                }
            }
        }
    }
    fn new(content: &Content) -> Option<Self> {
        match content {
            Content::ToolCard(call) => ToolCallCard::new((**call).clone())
                .ok()
                .map(Box::new)
                .map(Self::Single),
            Content::ToolGroup(group) => ToolGroupCard::new(group.clone()).ok().map(Self::Group),
            _ => None,
        }
    }
    pub(super) fn replace(&mut self, content: &Content) -> bool {
        match (self, content) {
            (Self::Single(card), Content::ToolCard(call)) => {
                card.call() == call.as_ref() || card.replace((**call).clone())
            }
            (Self::Group(card), Content::ToolGroup(group)) => card.replace(group.clone()),
            _ => false,
        }
    }
    pub(super) fn tick(&mut self, elapsed: Duration) -> bool {
        match self {
            Self::Single(card) => card.tick(elapsed),
            Self::Group(group) => group.tick(elapsed),
        }
    }
    pub(super) fn rows(
        &self,
        blocks: &[Block],
        identity: (&str, &str),
        width: u16,
        symbols: Symbols,
        force: bool,
        matched: Option<usize>,
    ) -> Vec<Row> {
        let (message, part) = identity;
        let projected = match self {
            Self::Single(card) => {
                if force {
                    card.rows_search(width.saturating_sub(2), symbols, matched)
                } else {
                    card.rows(width.saturating_sub(2), symbols, false)
                }
            }
            Self::Group(group) => {
                if let Some(matched) = matched {
                    let found = blocks
                        .iter()
                        .filter(|b| b.start <= matched)
                        .max_by_key(|b| b.start);
                    if let Some(Block {
                        start,
                        kind: super::content::Kind::Tool { id },
                        ..
                    }) = found
                    {
                        group.rows_search(width.saturating_sub(2), symbols, id, matched - start)
                    } else {
                        group.rows(width.saturating_sub(2), symbols, force)
                    }
                } else {
                    group.rows(width.saturating_sub(2), symbols, force)
                }
            }
        };
        projected
            .into_iter()
            .map(|row| {
                let start = match (self, row.call.as_deref()) {
                    (Self::Group(group), Some(call)) => group
                        .index(call)
                        .and_then(|index| blocks.get(index + 1))
                        .map_or(0, |b| b.start),
                    _ => 0,
                };
                let anchor = Anchor {
                    message: message.into(),
                    part: part.into(),
                    offset: start + row.offset,
                };
                Row {
                    anchor,
                    block: Some(BlockId {
                        message: message.into(),
                        part: part.into(),
                        start,
                    }),
                    prefix: format!("  {}", row.prefix),
                    runs: row
                        .pieces
                        .into_iter()
                        .map(|p| Run {
                            text: p.text,
                            ink: ink(p.role),
                            generated: p.generated,
                        })
                        .collect(),
                    surface: row.surface,
                    header: row.head,
                    searchable: true,
                    map: None,
                }
            })
            .collect()
    }
}
fn ink(role: crate::tool_call_card::view::Role) -> Ink {
    use crate::tool_call_card::view::Role;
    Ink {
        muted: role == Role::Dim,
        bold: matches!(role, Role::Command | Role::Error),
        info: role == Role::Info,
        link: role == Role::Link,
        success: matches!(role, Role::Success | Role::Add),
        error: matches!(role, Role::Error | Role::Delete),
        accent: role == Role::Accent,
        warning: role == Role::Warning,
        ..Ink::default()
    }
}
pub(super) fn valid(content: &Content) -> bool {
    match content {
        Content::ToolCard(call) => call.validate().is_ok(),
        Content::ToolGroup(group) => group.validate().is_ok(),
        _ => true,
    }
}
pub(super) fn valid_change(old: &Content, new: &Content) -> bool {
    if old == new {
        return true;
    }
    View::new(old).is_none_or(|mut view| view.replace(new))
}
impl Chat {
    pub(super) fn sync_tools(&mut self, key: &(String, String), content: &Content) {
        if let Some(view) = self.tools.get_mut(key) {
            view.replace(content);
        } else if let Some(view) = View::new(content) {
            self.tools.insert(key.clone(), view);
        }
        if !matches!(content, Content::ToolCard(_) | Content::ToolGroup(_)) {
            self.tools.remove(key);
        }
    }
    fn selected_tool(&self) -> Option<(BlockId, Option<String>)> {
        let id = self.selection.as_ref()?.last.clone();
        let content = &self
            .turn(&id.message)?
            .parts
            .iter()
            .find(|p| p.id == id.part)?
            .content;
        let call = match content {
            Content::ToolCard(call) => Some(call.id.clone()),
            Content::ToolGroup(group) => {
                let index = self
                    .cache
                    .get(&(id.message.clone(), id.part.clone()))?
                    .blocks
                    .iter()
                    .position(|b| b.start == id.start)?;
                index
                    .checked_sub(1)
                    .and_then(|i| group.calls.get(i))
                    .map(|c| c.id.clone())
            }
            _ => return None,
        };
        Some((id, call))
    }
    pub(super) fn tool_event(&mut self, event: &Event) -> bool {
        let Event::Key(key) = event else {
            return false;
        };
        if key.kind != KeyEventKind::Press || key.modifiers != KeyModifiers::NONE {
            return false;
        }
        if key.code == KeyCode::Char('a') {
            if let Some((id, Some(call))) = self.selected_tool() {
                self.artifacts = super::artifacts::Chooser::new(self, &id.message, &id.part, &call);
            }
            return true;
        }
        let command = match key.code {
            KeyCode::Enter => Command::Toggle,
            KeyCode::Char('o') => Command::Details,
            KeyCode::Char('r') => Command::Retry,
            KeyCode::Char('y') => Command::CopyCall,
            KeyCode::Char('c') => Command::CopyOutput,
            _ => return false,
        };
        if matches!(command, Command::CopyCall | Command::CopyOutput)
            && self
                .selection
                .as_ref()
                .is_some_and(|s| s.whole || s.first != s.last)
        {
            return false;
        }
        let Some((id, call)) = self.selected_tool() else {
            return false;
        };
        let entry = (id.message.clone(), id.part.clone());
        let Some(view) = self.tools.get_mut(&entry) else {
            return false;
        };
        let intent = match (view, call) {
            (View::Single(card), _) => card.command(command),
            (View::Group(group), Some(call)) => {
                group.card_mut(&call).and_then(|card| card.command(command))
            }
            (View::Group(group), None) => {
                if command == Command::Toggle {
                    group.toggle();
                }
                None
            }
        };
        if let Some(intent) = intent {
            self.intents.push(Intent::Tool {
                message: id.message.clone(),
                part: id.part.clone(),
                intent,
            });
        }
        if command == Command::Toggle {
            let position = self
                .rows
                .iter()
                .position(|r| r.row().block.as_ref() == Some(&id))
                .map(|row| row.saturating_sub(self.scroll));
            if let Some(cache) = self.cache.get_mut(&entry) {
                cache.width = 0;
            }
            self.refresh();
            if let Some(position) = position
                && let Some(row) = self
                    .rows
                    .iter()
                    .position(|r| r.row().block.as_ref() == Some(&id))
            {
                self.scroll = row.saturating_sub(position).min(self.maximum());
                self.capture_anchor();
            }
        }
        true
    }
    pub(super) fn tool_hint(&self) -> Option<String> {
        if self
            .selection
            .as_ref()
            .is_some_and(|s| s.whole || s.first != s.last)
        {
            return None;
        }
        let (id, call) = self.selected_tool()?;
        match (self.tools.get(&(id.message, id.part))?, call) {
            (View::Single(card), _) => Some(card.hint_for(self.width)),
            (View::Group(group), Some(call)) => {
                group.card(&call).map(|card| card.hint_for(self.width))
            }
            (View::Group(group), None) => Some(format!(
                "enter {} group · esc done",
                if group.expanded() {
                    "collapse"
                } else {
                    "expand"
                }
            )),
        }
    }
    /// Deliberately open a supplied artifact by stable identity; emits an acknowledged host intent.
    pub fn open_tool_artifact(
        &mut self,
        message: &str,
        part: &str,
        call: &str,
        artifact: &str,
    ) -> bool {
        let Some(view) = self.tools.get_mut(&(message.into(), part.into())) else {
            return false;
        };
        let card = match view {
            View::Single(card) if card.call().id == call => Some(card.as_mut()),
            View::Group(group) => group.card_mut(call),
            View::Single(_) => None,
        };
        let Some(card) = card else {
            return false;
        };
        let Some(index) = card.call().artifacts.iter().position(|a| a.id == artifact) else {
            return false;
        };
        let Some(intent) = card.command(Command::OpenArtifact(index)) else {
            return false;
        };
        self.intents.push(Intent::Tool {
            message: message.into(),
            part: part.into(),
            intent,
        });
        true
    }
    /// Acknowledge an identified card effect; stale/foreign/evicted tickets do nothing.
    pub fn acknowledge_tool(
        &mut self,
        message: &str,
        part: &str,
        call: &str,
        ticket: &Ticket,
        result: Result<&str, &str>,
    ) -> bool {
        let Some(view) = self.tools.get_mut(&(message.into(), part.into())) else {
            return false;
        };
        let card = match view {
            View::Single(card) if card.call().id == call => Some(card.as_mut()),
            View::Group(group) => group.card_mut(call),
            View::Single(_) => None,
        };
        let Some(card) = card else {
            return false;
        };
        if !card.acknowledge(ticket, result) {
            return false;
        }
        let notice = card.notice().to_owned();
        self.say(&notice);
        true
    }
    /// Accept a retry as a new card immediately below its failed/cancelled parent.
    /// Rejects stale tickets and invalid/duplicate successor IDs atomically.
    pub fn complete_tool_retry(
        &mut self,
        message: &str,
        part: &str,
        call: &str,
        ticket: &Ticket,
        successor: &crate::tool_call_card::Call,
    ) -> bool {
        use crate::tool_call_card::State;
        let key = (message.to_owned(), part.to_owned());
        let Some(view) = self.tools.get(&key) else {
            return false;
        };
        let card = match view {
            View::Single(card) => Some(card.as_ref()),
            View::Group(group) => group.card(call),
        };
        if card.is_none_or(|card| !card.pending_retry(ticket))
            || successor.validate().is_err()
            || !matches!(successor.state, State::Queued | State::Running)
            || successor.retried.is_some()
            || successor.id == call
        {
            return false;
        }
        let Some(record) = self.messages.iter().find(|m| m.turn.id == message) else {
            return false;
        };
        if (record.turn.parts.len() >= super::PART_LIMIT && matches!(view, View::Single(_)))
            || record.turn.parts.iter().any(|p| match &p.content {
                Content::ToolCard(c) => c.id == successor.id,
                Content::ToolGroup(g) => g.calls.iter().any(|c| c.id == successor.id),
                _ => false,
            })
        {
            return false;
        }
        let Some(index) = record.turn.parts.iter().position(|p| p.id == part) else {
            return false;
        };
        let mut parent = record.turn.parts[index].clone();
        let Some(revision) = parent.revision.checked_add(1) else {
            return false;
        };
        parent.revision = revision;
        match &mut parent.content {
            Content::ToolCard(old) if old.id == call => {
                let Some(revision) = old.revision.checked_add(1) else {
                    return false;
                };
                old.retried = Some(successor.id.clone());
                old.revision = revision;
            }
            Content::ToolGroup(group) => {
                if group.calls.len() >= 128 {
                    return false;
                }
                let Some(i) = group.calls.iter().position(|c| c.id == call) else {
                    return false;
                };
                let (Some(revision), Some(group_revision)) = (
                    group.calls[i].revision.checked_add(1),
                    group.revision.checked_add(1),
                ) else {
                    return false;
                };
                group.calls[i].retried = Some(successor.id.clone());
                group.calls[i].revision = revision;
                group.calls.insert(i + 1, successor.clone());
                group.revision = group_revision;
            }
            _ => return false,
        }
        let Some(next_part) = (0..=super::PART_LIMIT)
            .map(|i| format!("tool-retry-{i}"))
            .find(|id| record.turn.parts.iter().all(|p| p.id != *id))
        else {
            return false;
        };
        // The ticket is consumed only after every mutation has been validated.
        if !self.acknowledge_tool(
            message,
            part,
            call,
            ticket,
            Ok("retry started · previous call kept"),
        ) {
            return false;
        }
        let Some(record) = self.messages.iter_mut().find(|m| m.turn.id == message) else {
            return false;
        };
        let grouped = matches!(parent.content, Content::ToolGroup(_));
        record.turn.parts[index] = parent;
        let selected_part = if grouped {
            part.to_owned()
        } else {
            record.turn.parts.insert(
                index + 1,
                super::Part::new(&next_part, Content::ToolCard(Box::new(successor.clone()))),
            );
            next_part
        };
        record.bound();
        self.refresh();
        self.select_retry(message, part, &successor.id, selected_part, grouped);
        true
    }
    fn select_retry(
        &mut self,
        message: &str,
        part: &str,
        successor: &str,
        selected_part: String,
        grouped: bool,
    ) {
        let key = (message.to_owned(), part.to_owned());
        let start = if grouped {
            let group = self.tools.get(&(message.into(), part.into()));
            if let Some(View::Group(group)) = group {
                group
                    .index(successor)
                    .and_then(|i| self.cache.get(&key).and_then(|c| c.blocks.get(i + 1)))
                    .map_or(0, |b| b.start)
            } else {
                0
            }
        } else {
            0
        };
        let id = BlockId {
            message: message.into(),
            part: selected_part,
            start,
        };
        if let Some(selection) = &mut self.selection {
            selection.first = id.clone();
            selection.last = id.clone();
            selection.whole = false;
        }
        self.reveal_tool(&id);
    }
    fn reveal_tool(&mut self, id: &BlockId) {
        if let Some(row) = self
            .rows
            .iter()
            .position(|r| r.row().block.as_ref() == Some(id))
        {
            if row < self.scroll || row >= self.scroll + self.height {
                self.scroll = row.saturating_sub(2).min(self.maximum());
            }
            self.capture_anchor();
        }
    }
}
