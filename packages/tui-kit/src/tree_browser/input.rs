//! Scoped keyboard/editor/menu routing; delivered host work stays host-owned.

use super::{Children, Cursor, Intent, Menu, Operation, Placement, TreeBrowser};
use crate::{
    event::{Event, KeyCode, KeyEventKind, KeyModifiers},
    picker::{Choice, ChoicePicker, Filter, Intent as Pick, SelectionMode},
};
impl TreeBrowser {
    /// Route one caller event. Unfocused/closed trees consume nothing.
    pub fn update(&mut self, event: &Event) -> bool {
        if self.closed || !self.focused {
            return false;
        }
        if self.menu.is_some() {
            return self.menu_event(event);
        }
        let Event::Key(k) = event else {
            if self.filtering {
                self.filter_event(event);
                return true;
            }
            return false;
        };
        if k.kind == KeyEventKind::Release {
            return false;
        }
        if k.modifiers == KeyModifiers::ALT {
            match k.code {
                KeyCode::PageDown => self.details_offset = self.details_offset.saturating_add(3),
                KeyCode::PageUp => self.details_offset = self.details_offset.saturating_sub(3),
                _ => return false,
            }
            return true;
        }
        if self.filtering {
            self.filter_key(event, k);
            return true;
        }
        if k.modifiers == KeyModifiers::CONTROL {
            match k.code {
                KeyCode::Char('p') => self.move_key(KeyCode::Up),
                KeyCode::Char('n') => self.move_key(KeyCode::Down),
                _ => return false,
            }
            return true;
        }
        if !k.modifiers.is_empty() {
            return false;
        }
        match k.code {
            KeyCode::Up
            | KeyCode::Down
            | KeyCode::Home
            | KeyCode::End
            | KeyCode::PageUp
            | KeyCode::PageDown => self.move_key(k.code),
            KeyCode::Left => self.left(),
            KeyCode::Right => self.right(),
            KeyCode::Char(' ') => self.toggle(),
            KeyCode::Char('*') if self.query.text().is_empty() => {
                if let Some(n) = self.selected_node() {
                    let id = n.id.clone();
                    self.cascade.insert(id);
                    self.expand_cascades();
                }
            }
            KeyCode::Char('-') if self.query.text().is_empty() => {
                if let Some(n) = self.selected_node() {
                    let id = n.id.clone();
                    self.collapse(&id, true);
                }
            }
            KeyCode::Char('/') => self.filtering = true,
            KeyCode::Enter if k.kind == KeyEventKind::Press => self.enter(),
            KeyCode::Char('a') if k.kind == KeyEventKind::Press => self.open_menu(),
            KeyCode::Tab if self.placement == Placement::Panel => {
                self.focused = false;
                self.intents.push(Intent::MainView);
            }
            KeyCode::Esc if k.kind == KeyEventKind::Press => {
                if !self.query.text().is_empty() {
                    let ancestors = self.ancestry();
                    self.query.set_text("");
                    self.repair(&ancestors);
                } else if self.placement == Placement::Panel {
                    self.focused = false;
                    self.intents.push(Intent::MainView);
                } else {
                    self.close();
                    self.intents.push(Intent::Returned);
                }
            }
            KeyCode::Char(c) if k.kind == KeyEventKind::Press => {
                if let Some(n) = self.selected_node() {
                    let found = n
                        .actions
                        .iter()
                        .find(|a| a.shortcut == Some(c))
                        .map(|a| (n.id.clone(), a.id.clone()));
                    if let Some((id, action)) = found {
                        self.action(&id, &action);
                    } else {
                        return false;
                    }
                }
            }
            _ => return false,
        }
        true
    }
    fn filter_key(&mut self, event: &Event, k: &crate::event::KeyEvent) {
        match k.code {
            KeyCode::Char('p' | 'n') if k.modifiers == KeyModifiers::CONTROL => {
                self.move_key(if k.code == KeyCode::Char('p') {
                    KeyCode::Up
                } else {
                    KeyCode::Down
                });
            }
            KeyCode::Esc if k.kind == KeyEventKind::Press && k.modifiers.is_empty() => {
                let ancestors = self.ancestry();
                self.query.set_text("");
                self.filtering = false;
                self.repair(&ancestors);
            }
            KeyCode::Up | KeyCode::Down | KeyCode::PageUp | KeyCode::PageDown
                if k.modifiers.is_empty() =>
            {
                self.move_key(k.code);
            }
            KeyCode::Enter if k.kind == KeyEventKind::Press && k.modifiers.is_empty() => {
                self.enter();
            }
            _ => self.filter_event(event),
        }
    }
    fn filter_event(&mut self, event: &Event) {
        let ancestors = self.ancestry();
        let old = self.query.text().to_owned();
        if matches!(event, Event::Paste(s) if s.contains(['\r','\n']))
            || matches!(event, Event::Key(k) if matches!(k.code,KeyCode::Enter) || (k.code==KeyCode::Char('j') && k.modifiers.contains(KeyModifiers::CONTROL)))
        {
            return;
        }
        self.query.update(event);
        if self.query.text() != old {
            self.offset = 0;
            self.repair(&ancestors);
        }
    }
    fn move_key(&mut self, key: KeyCode) {
        let projected = self.rows();
        if matches!(key, KeyCode::PageUp | KeyCode::PageDown) {
            let index = projected
                .iter()
                .position(|r| r.key.as_ref() == self.cursor.as_ref() && r.key.is_some())
                .unwrap_or(0);
            let step = self.viewport.saturating_sub(1).max(1);
            let target = if key == KeyCode::PageUp {
                index.saturating_sub(step)
            } else {
                index
                    .saturating_add(step)
                    .min(projected.len().saturating_sub(1))
            };
            let mut forward = projected.iter().skip(target).filter_map(|r| r.key.clone());
            let mut backward = projected
                .iter()
                .take(target + 1)
                .rev()
                .filter_map(|r| r.key.clone());
            self.cursor = if key == KeyCode::PageDown {
                forward.next().or_else(|| backward.next())
            } else {
                backward.next().or_else(|| forward.next())
            };
            self.details_offset = 0;
            return;
        }
        let rows: Vec<_> = projected.into_iter().filter_map(|r| r.key).collect();
        let index = self
            .cursor
            .as_ref()
            .and_then(|c| rows.iter().position(|r| r == c))
            .unwrap_or(0);
        let next = match key {
            KeyCode::Home => 0,
            KeyCode::End => rows.len().saturating_sub(1),
            KeyCode::Up => index.saturating_sub(1),
            _ => index.saturating_add(1),
        };
        self.cursor = rows.get(next.min(rows.len().saturating_sub(1))).cloned();
        self.details_offset = 0;
    }
    fn right(&mut self) {
        let Some(n) = self.selected_node() else {
            return;
        };
        let id = n.id.clone();
        if !self.folded.contains(&id) && self.query.text().is_empty() {
            self.open_branch(&id);
        } else if let Some(child) = self
            .rows()
            .into_iter()
            .find(|r| r.parent.as_deref() == Some(&id) && r.key.is_some())
        {
            self.cursor = child.key;
        }
    }
    fn left(&mut self) {
        match self.cursor.clone() {
            Some(Cursor::Node(id)) => {
                if self.folded.contains(&id) && self.query.text().is_empty() {
                    self.collapse(&id, false);
                } else {
                    self.cursor = self
                        .node(&id)
                        .and_then(|n| n.parent.as_ref())
                        .map(|id| Cursor::Node(id.clone()))
                        .or_else(|| self.cursor.clone());
                }
            }
            Some(Cursor::Children(Some(id))) => self.cursor = Some(Cursor::Node(id)),
            _ => {}
        }
    }
    fn toggle(&mut self) {
        if let Some(n) = self.selected_node() {
            let id = n.id.clone();
            if self.folded.contains(&id) {
                self.collapse(&id, false);
            } else if self.query.text().is_empty() {
                self.open_branch(&id);
            }
        }
    }
    fn enter(&mut self) {
        match self.cursor.clone() {
            Some(Cursor::Node(id)) => {
                if let Some(a) = self.node(&id).and_then(|n| n.primary.clone()) {
                    self.action(&id, &a);
                }
            }
            Some(Cursor::Children(id)) => {
                let cursor = id.as_deref().map_or_else(
                    || self.snapshot.more.clone(),
                    |id| self.node(id).and_then(|n| n.more.clone()),
                );
                self.load(id, cursor);
            }
            None => {}
        }
    }
    fn open_menu(&mut self) {
        let Some(n) = self.selected_node() else {
            return;
        };
        if n.actions.is_empty() || self.pending.contains_key(&Some(n.id.clone())) {
            return;
        }
        let choices = n
            .actions
            .iter()
            .map(|a| {
                let mut c = Choice::new(&a.id, &a.label);
                c.disabled.clone_from(&a.disabled);
                c.detail = a.shortcut.map_or_else(String::new, |c| c.to_string());
                c
            })
            .collect();
        if let Ok(picker) = ChoicePicker::new(choices, None, SelectionMode::Single, Filter::Off) {
            self.menu = Some(Menu {
                node: n.id.clone(),
                revision: n.revision,
                picker,
            });
        }
    }
    fn menu_event(&mut self, event: &Event) -> bool {
        let Some(menu) = &mut self.menu else {
            return false;
        };
        match menu.picker.update(event) {
            Pick::Selected(action) => {
                let id = menu.node.clone();
                self.menu = None;
                self.action(&id, &action);
            }
            Pick::Confirmed(ids) => {
                let id = menu.node.clone();
                let action = ids.first().cloned();
                self.menu = None;
                if let Some(action) = action {
                    self.action(&id, &action);
                }
            }
            Pick::Cancelled => self.menu = None,
            _ => {}
        }
        true
    }
    fn auxiliary(&self, id: Option<&str>, depth: usize, out: &mut Vec<super::Row>) {
        let parent = id.map(str::to_owned);
        let key = Cursor::Children(parent.clone());
        let (state, more) = id
            .and_then(|id| self.node(id))
            .map_or((&self.snapshot.roots, self.snapshot.more.as_ref()), |n| {
                (&n.children, n.more.as_ref())
            });
        let pending = self
            .pending
            .get(&parent)
            .filter(|p| matches!(p.request.operation, Operation::Load { .. }));
        let failure = self.load_failures.get(&parent).or({
            if let Children::Failed(s) = state {
                Some(s)
            } else {
                None
            }
        });
        let (text, selectable) = if let Some(p) = pending {
            (
                format!(
                    "loading · {}s · attempt {}",
                    p.elapsed.as_secs(),
                    self.attempts.get(&parent).copied().unwrap_or(1)
                ),
                self.cursor.as_ref() == Some(&key),
            )
        } else if let Some(s) = failure {
            (format!("couldn't load: {s} · enter retries"), true)
        } else if let Children::Loading(s) = state {
            (format!("loading: {s}"), false)
        } else if matches!(state, Children::Unloaded) {
            ("not loaded · enter loads".into(), true)
        } else if more.is_some() {
            ("more · enter loads next page".into(), true)
        } else if !self
            .snapshot
            .nodes
            .iter()
            .any(|n| n.parent.as_deref() == id)
        {
            ("no children".into(), false)
        } else {
            return;
        };
        out.push(super::Row {
            key: selectable.then_some(key),
            parent,
            depth,
            text,
        });
    }
    pub(super) fn rows(&self) -> Vec<super::Row> {
        enum Entry {
            Node(String, usize),
            Auxiliary(Option<String>, usize),
        }
        let query = self.query.text().to_lowercase();
        let mut keep = std::collections::HashSet::new();
        if !query.is_empty() {
            for n in self
                .snapshot
                .nodes
                .iter()
                .filter(|n| n.name.to_lowercase().contains(&query))
            {
                let mut cursor = Some(n.id.as_str());
                while let Some(id) = cursor {
                    keep.insert(id.to_owned());
                    cursor = self.node(id).and_then(|n| n.parent.as_deref());
                }
            }
        }
        let mut out = vec![];
        let mut stack = vec![Entry::Auxiliary(None, 0)];
        for n in self
            .snapshot
            .nodes
            .iter()
            .rev()
            .filter(|n| n.parent.is_none() && (query.is_empty() || keep.contains(&n.id)))
        {
            stack.push(Entry::Node(n.id.clone(), 0));
        }
        while let Some(entry) = stack.pop() {
            match entry {
                Entry::Auxiliary(id, depth) => {
                    if query.is_empty() {
                        self.auxiliary(id.as_deref(), depth, &mut out);
                    }
                }
                Entry::Node(id, depth) => {
                    let Some(n) = self.node(&id) else {
                        continue;
                    };
                    out.push(super::Row {
                        key: Some(Cursor::Node(id.clone())),
                        parent: n.parent.clone(),
                        depth,
                        text: String::new(),
                    });
                    if let Some(reason) = self.failures.get(&Some(id.clone())) {
                        out.push(super::Row {
                            key: None,
                            parent: Some(id.clone()),
                            depth: depth + 1,
                            text: format!("failed: {reason}"),
                        });
                    }
                    if n.has_children && (!query.is_empty() || self.folded.contains(&id)) {
                        stack.push(Entry::Auxiliary(Some(id.clone()), depth + 1));
                        for n in self.snapshot.nodes.iter().rev().filter(|n| {
                            n.parent.as_deref() == Some(&id)
                                && (query.is_empty() || keep.contains(&n.id))
                        }) {
                            stack.push(Entry::Node(n.id.clone(), depth + 1));
                        }
                    }
                }
            }
        }
        out
    }
}
