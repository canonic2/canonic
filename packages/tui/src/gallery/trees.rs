//! Deterministic tree host, retained Chat and responsive inspection composition.

use crate::{
    chat::{Anchor, Chat, Intent as ChatIntent, Role, Turn},
    event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers},
    picker::{Choice, ChoicePicker, Filter, Intent as Pick, PickerView, SelectionMode},
    theme::Presentation,
    tree_browser::{
        Action, Children, Cursor, Intent, Kind, Node, Operation, Outcome, Placement, Request,
        Snapshot, State, TreeBrowser,
    },
};
use ratatui::{Frame, Terminal, backend::TestBackend, layout::Rect, widgets::Paragraph};
use std::time::Duration;
pub(super) const NAMES: &[&str] = &[
    "main",
    "files",
    "tasks",
    "cursor-open",
    "unfocused",
    "long-names",
    "statuses",
    "disabled",
    "deep",
    "paging",
    "progress",
    "outcomes",
    "filter",
    "unloaded",
    "loading",
    "loaded",
    "empty",
    "load-failed",
    "retrying",
    "refreshed",
    "removed",
    "ancestor-removed",
    "bad-data",
    "root-loading",
    "root-failed",
    "root-retry",
    "open-folder",
    "first-child",
    "leaf-parent",
    "close-folder",
    "expand-all",
    "scroll",
    "toggle",
    "collapse-all",
    "filter-clear",
    "opening",
    "opened",
    "open-failed",
    "selecting",
    "actions",
    "stopping",
    "action-done",
    "action-failed",
    "stale-result",
    "foreign-result",
    "duplicate-result",
    "material-changed",
    "concurrent",
    "late-collapsed",
    "no-ack",
    "delayed",
    "full-screen",
    "panel",
    "medium",
    "narrow",
    "tiny",
    "before-chat",
    "returning",
    "palette",
    "narrow-filter",
    "long-error",
    "paste-isolated",
    "ascii",
    "no-color",
    "light",
    "reduced-motion",
];
fn key(code: KeyCode) -> Event {
    Event::Key(KeyEvent::new(code, KeyModifiers::NONE))
}
#[expect(
    clippy::too_many_lines,
    reason = "Bounded file and task descriptors preserve the reference tree's host-supplied order and facts"
)]
pub(super) fn snapshot(tasks: bool) -> Snapshot {
    let mut nodes = vec![];
    for (id, name, parent, branch, state, detail) in [
        ("src", "src", None, true, State::Plain, "4 changed"),
        (
            "digest",
            "digest",
            Some("src"),
            true,
            State::Plain,
            "3 changed",
        ),
        (
            "settings",
            "settings.ts",
            Some("digest"),
            false,
            State::Modified,
            "+6 -2",
        ),
        (
            "schedule",
            "schedule.ts",
            Some("digest"),
            false,
            State::Modified,
            "+3 -1",
        ),
        (
            "timezone",
            "timezone.ts",
            Some("digest"),
            false,
            State::Added,
            "+41",
        ),
        (
            "render",
            "render.tsx",
            Some("digest"),
            false,
            State::Plain,
            "",
        ),
        (
            "profile",
            "profile",
            Some("src"),
            true,
            State::Plain,
            "1 changed",
        ),
        ("billing", "billing", Some("src"), true, State::Plain, ""),
        ("test", "test", None, true, State::Plain, "1 changed"),
        (
            "package",
            "package.json",
            None,
            false,
            State::Modified,
            "+1 -1",
        ),
        ("readme", "README.md", None, false, State::Plain, ""),
    ] {
        let mut n = Node::new(id, name, parent);
        n.has_children = branch;
        n.kind = if branch { Kind::Directory } else { Kind::File };
        n.state = state;
        n.detail = detail.into();
        if matches!(id, "profile" | "billing" | "test") {
            n.children = Children::Unloaded;
        }
        let mut open = Action::new("open", "open diff");
        open.opens = true;
        open.pending = "opening".into();
        n.primary = Some("open".into());
        n.actions.push(open);
        nodes.push(n);
    }
    if tasks {
        for (n, name, state, detail) in [
            (
                "src",
                "Make digests timezone-aware",
                State::Working,
                "3 of 4 · 4m",
            ),
            (
                "digest",
                "run tests",
                State::Working,
                "12/40 · [###-------] · 1m",
            ),
            ("settings", "read settings", State::Done, "0.2s"),
            (
                "schedule",
                "digest-schedule.test.ts",
                State::Working,
                "running",
            ),
            ("timezone", "digest-render.test.ts", State::Queued, "queued"),
            ("render", "open pull request", State::Queued, "queued"),
        ] {
            if let Some(row) = nodes.iter_mut().find(|row| row.id == n) {
                row.kind = Kind::Task;
                row.name = name.into();
                row.state = state;
                row.detail = detail.into();
            }
        }
        nodes.retain(|n| {
            matches!(
                n.id.as_str(),
                "src" | "digest" | "settings" | "schedule" | "timezone" | "render"
            )
        });
        for n in &mut nodes {
            n.kind = Kind::Task;
            n.actions[0].label = "show task".into();
            n.actions[0].pending = "selecting".into();
            let mut stop = Action::new("stop", "stop this task");
            stop.shortcut = Some('x');
            stop.pending = "stopping".into();
            n.actions.push(stop);
            let mut retry = Action::new("retry", "retry");
            retry.shortcut = Some('r');
            retry.disabled = Some("only after it ends".into());
            n.actions.push(retry);
        }
    }
    Snapshot {
        revision: 1,
        title: if tasks { "tasks" } else { "files" }.into(),
        summary: "orbit-api · host supplied".into(),
        nodes,
        roots: Children::Ready,
        more: None,
    }
}
pub(super) struct TreeDemo {
    tree: TreeBrowser,
    chat: Chat,
    inspection: Chat,
    file_inspection: Option<crate::inspection::Inspector>,
    requests: Vec<Request>,
    last: Option<Request>,
    palette: Option<ChoicePicker>,
    fixture: usize,
    showing: bool,
    panel: bool,
    delayed: bool,
    elapsed: Duration,
    log: String,
}
impl TreeDemo {
    fn inspection(id: &str, task: bool) -> Chat {
        let mut chat = Chat::new();
        chat.set_files_available(true);
        chat.set_footer(&format!("{id} ~inspection fixture"));
        let body = if task {
            format!(
                "Inspection fixture for task {id}\n\n```text\n12 / 40 tests\ndigest-settings.test.ts passed\ndigest-schedule.test.ts running\n```"
            )
        } else {
            format!(
                "Inspection fixture for {id}\n\n```diff\n@@ -18,2 +18,2 @@\n-  json,\n+  json, timezone: user.timezone\n```"
            )
        };
        chat.push_record(Turn::new("inspection", Role::Assistant, "acme", &body));
        chat
    }
    #[expect(
        clippy::too_many_lines,
        reason = "Each named tree fixture constructs one bounded, executable host interaction"
    )]
    pub(super) fn new(fixture: usize) -> Self {
        let name = NAMES[fixture % NAMES.len()];
        let tasks = matches!(
            name,
            "tasks"
                | "progress"
                | "outcomes"
                | "selecting"
                | "actions"
                | "stopping"
                | "action-done"
                | "action-failed"
                | "reduced-motion"
        );
        let mut s = snapshot(tasks);
        if name == "empty" {
            s.nodes.clear();
        }
        if matches!(name, "root-loading" | "root-failed" | "root-retry") {
            s.nodes.clear();
            s.roots = if name == "root-loading" {
                Children::Loading("listing changes".into())
            } else {
                Children::Failed("git exited 128".into())
            };
        }
        if name == "bad-data" {
            s.nodes.push(s.nodes[0].clone());
            let mut cycle = Node::new("cycle", "loop", Some("cycle"));
            cycle.has_children = true;
            s.nodes.push(cycle);
        }
        if name == "long-names" {
            s.nodes[3].name =
                "very-long-digest-schedule-with-combining-e\u{301}-and-界-rendering.test.ts".into();
        }
        if name == "disabled" {
            s.nodes[3].actions[0].disabled = Some("unavailable: file was removed".into());
        }
        if name == "statuses" {
            for (i, state) in [
                State::Modified,
                State::Added,
                State::Deleted,
                State::Renamed,
                State::Untracked,
                State::Ignored,
            ]
            .iter()
            .enumerate()
            {
                let mut n = Node::new(
                    &format!("status-{i}"),
                    &format!("status-{state:?}.ts"),
                    Some("digest"),
                );
                n.state = *state;
                n.detail = if *state == State::Renamed {
                    "from old-name.ts".into()
                } else {
                    String::new()
                };
                s.nodes.push(n);
            }
        }
        if name == "outcomes" {
            for (i, state) in [
                State::Done,
                State::Failed,
                State::Cancelled,
                State::Interrupted,
            ]
            .iter()
            .enumerate()
            {
                let mut n = Node::new(
                    &format!("outcome-{i}"),
                    &format!("task {state:?}"),
                    Some("digest"),
                );
                n.kind = Kind::Task;
                n.state = *state;
                n.detail = format!("{state:?}");
                s.nodes.push(n);
            }
        }
        if name == "deep" {
            for i in 0..18 {
                let parent = if i == 0 {
                    "digest".into()
                } else {
                    format!("deep-{}", i - 1)
                };
                let mut n = Node::new(&format!("deep-{i}"), &format!("level-{i}"), Some(&parent));
                n.has_children = true;
                n.kind = Kind::Directory;
                s.nodes.push(n);
            }
        }
        if name == "scroll" {
            for i in 0..80 {
                s.nodes.push(Node::new(
                    &format!("long-{i}"),
                    &format!("file-{i:02}.ts"),
                    Some("digest"),
                ));
            }
        }
        if name == "paging" {
            s.nodes[1].more = Some("page-2".into());
        }
        let panel = matches!(name, "main" | "panel" | "unfocused" | "medium" | "tasks");
        let mut tree = TreeBrowser::new(
            s,
            if panel {
                Placement::Panel
            } else {
                Placement::FullScreen
            },
        )
        .expect("valid tree fixture");
        tree.update(&key(KeyCode::Right));
        tree.update(&key(KeyCode::Right));
        tree.update(&key(KeyCode::Right));
        tree.select(Cursor::Node("schedule".into()));
        let _ = tree.set_opened(Some("settings"));
        let mut chat = Chat::new();
        chat.set_files_available(true);
        chat.set_input("and then inspect the change");
        for _ in 0..6 {
            chat.composer_mut().update(&key(KeyCode::Left));
        }
        for i in 0..16 {
            chat.push_record(Turn::new(
                &format!("tree-chat-{i}"),
                Role::Assistant,
                "acme",
                &format!("Inspection fixture line {i}. Host supplies this content."),
            ));
        }
        chat.jump(Anchor {
            message: "tree-chat-2".into(),
            part: "body".into(),
            offset: 0,
        });
        let mut demo = Self {
            tree,
            chat,
            inspection: Self::inspection("settings", tasks),
            file_inspection: if tasks {
                None
            } else {
                Some(super::inspection::viewer(
                    "settings",
                    crate::inspection::Origin::Tree,
                ))
            },
            requests: vec![],
            last: None,
            palette: None,
            fixture,
            showing: true,
            panel,
            delayed: name == "delayed",
            elapsed: Duration::ZERO,
            log: String::new(),
        };
        match name {
            "unfocused" => {
                demo.tree.set_focused(false);
            }
            "deep" | "scroll" => {
                demo.tree.select(Cursor::Node("digest".into()));
                demo.tree.update(&key(KeyCode::Char('*')));
                if name == "scroll" {
                    demo.tree.update(&key(KeyCode::End));
                } else {
                    demo.tree.select(Cursor::Node("deep-17".into()));
                }
            }
            "paging" => {
                demo.tree.select(Cursor::Children(Some("digest".into())));
            }
            "filter" | "narrow-filter" | "paste-isolated" => {
                demo.tree.update(&key(KeyCode::Char('/')));
                demo.tree.update(&Event::Paste("sch".into()));
            }
            "filter-clear" => {
                demo.tree.update(&key(KeyCode::Char('/')));
                demo.tree.update(&Event::Paste("sch".into()));
                demo.tree.update(&key(KeyCode::Esc));
            }
            "unloaded" | "loading" | "loaded" | "load-failed" | "retrying" | "late-collapsed"
            | "long-error" => {
                demo.tree.select(Cursor::Node("profile".into()));
                if name != "unloaded" {
                    demo.tree.update(&key(KeyCode::Right));
                    demo.drain();
                }
                if name == "loaded" {
                    demo.reply(true);
                }
                if matches!(name, "load-failed" | "retrying" | "long-error") {
                    demo.reply(false);
                    demo.tree.select(Cursor::Children(Some("profile".into())));
                    if name == "retrying" {
                        demo.tree.update(&key(KeyCode::Enter));
                    }
                }
                if name == "late-collapsed" {
                    demo.tree.update(&key(KeyCode::Left));
                    demo.reply(true);
                }
            }
            "root-retry" => {
                demo.tree.update(&key(KeyCode::Enter));
            }
            "refreshed" => demo.refresh(false),
            "removed" => demo.remove(),
            "ancestor-removed" => {
                demo.tree.select(Cursor::Node("digest".into()));
                demo.remove();
            }
            "open-folder" | "first-child" | "close-folder" | "toggle" | "leaf-parent" => {
                if name == "leaf-parent" {
                    demo.tree.update(&key(KeyCode::Left));
                } else {
                    demo.tree.select(Cursor::Node("digest".into()));
                    demo.tree.update(&key(KeyCode::Left));
                    demo.tree.update(&key(if name == "toggle" {
                        KeyCode::Char(' ')
                    } else {
                        KeyCode::Right
                    }));
                    if name == "first-child" {
                        demo.tree.update(&key(KeyCode::Right));
                    }
                    if name == "close-folder" {
                        demo.tree.update(&key(KeyCode::Left));
                    }
                }
            }
            "expand-all" => {
                demo.tree.select(Cursor::Node("src".into()));
                demo.tree.update(&key(KeyCode::Char('*')));
            }
            "collapse-all" => {
                demo.tree.select(Cursor::Node("src".into()));
                demo.tree.update(&key(KeyCode::Char('-')));
            }
            "opening" | "opened" | "open-failed" | "selecting" | "no-ack" | "delayed"
            | "stale-result" | "foreign-result" | "duplicate-result" | "material-changed"
            | "concurrent" | "reduced-motion" => {
                demo.tree.update(&key(KeyCode::Enter));
                demo.drain();
                if matches!(name, "opened" | "duplicate-result" | "stale-result") {
                    demo.reply(true);
                }
                if name == "open-failed" {
                    demo.reply(false);
                }
                if name == "material-changed" {
                    demo.refresh(true);
                }
                if name == "concurrent" {
                    demo.tree.select(Cursor::Node("timezone".into()));
                    demo.tree.update(&key(KeyCode::Enter));
                }
                if matches!(name, "stale-result" | "duplicate-result") {
                    demo.stale();
                }
                if name == "foreign-result" {
                    let mut foreign =
                        TreeBrowser::new(snapshot(false), Placement::FullScreen).expect("fixture");
                    foreign.update(&key(KeyCode::Enter));
                    if let Some(Intent::Request(r)) = foreign.take_intents().pop() {
                        demo.log = format!(
                            "foreign result rejected: {}",
                            !demo.tree.resolve(&r, Outcome::Opened)
                        );
                    }
                }
            }
            "actions" => {
                demo.tree.update(&key(KeyCode::Char('a')));
            }
            "stopping" | "action-done" | "action-failed" => {
                demo.tree.update(&key(KeyCode::Char('x')));
                demo.drain();
                if name != "stopping" {
                    demo.reply(name == "action-done");
                }
            }
            "before-chat" | "returning" => {
                if name == "returning" {
                    demo.tree.update(&key(KeyCode::Esc));
                    demo.drain();
                } else {
                    demo.showing = false;
                }
            }
            "palette" => {
                demo.showing = false;
                demo.open_palette();
            }
            _ => {}
        }
        demo.drain();
        demo
    }
    fn drain(&mut self) {
        for intent in self.tree.take_intents() {
            match intent {
                Intent::Request(r) => self.requests.push(r),
                Intent::Returned => {
                    self.showing = false;
                    if let Some(viewer) = &mut self.file_inspection {
                        viewer.close();
                    }
                }
                Intent::MainView => {
                    if let Some(viewer) = &mut self.file_inspection {
                        viewer.reopen(crate::inspection::Origin::Tree);
                    }
                }
                Intent::Opened(id) => {
                    self.log = format!("host opened {id}");
                    let task = self
                        .tree
                        .snapshot()
                        .nodes
                        .iter()
                        .any(|n| n.id == id && n.kind == Kind::Task);
                    self.inspection = Self::inspection(&id, task);
                    self.file_inspection = if task {
                        None
                    } else {
                        Some(super::inspection::viewer(
                            &id,
                            crate::inspection::Origin::Tree,
                        ))
                    };
                }
            }
        }
        self.requests.retain(|request| {
            if self.tree.pending(request) {
                true
            } else {
                self.last = Some(request.clone());
                false
            }
        });
    }
    fn reply(&mut self, success: bool) {
        self.drain();
        let Some(r) = self.requests.first().cloned() else {
            return;
        };
        self.requests.remove(0);
        self.last = Some(r.clone());
        let outcome = if success {
            match &r.operation {
                Operation::Load { cursor } => {
                    let mut s = self.tree.snapshot().clone();
                    s.revision += 1;
                    if let Some(id) = &r.node {
                        if let Some(n) = s.nodes.iter_mut().find(|n| n.id == *id) {
                            n.children = Children::Ready;
                            n.more = None;
                        }
                        let node = Node::new(
                            &format!("loaded-{}", s.revision),
                            if cursor.is_some() {
                                "next-page.ts"
                            } else {
                                "api.ts"
                            },
                            Some(id),
                        );
                        s.nodes.push(node);
                    } else {
                        s.nodes = snapshot(false).nodes;
                        s.roots = Children::Ready;
                        s.more = None;
                    }
                    Outcome::Loaded(s)
                }
                Operation::Action(action) => {
                    if action == "open" {
                        Outcome::Opened
                    } else {
                        let mut s = self.tree.snapshot().clone();
                        s.revision += 1;
                        if let Some(n) = s.nodes.iter_mut().find(|n| Some(&n.id) == r.node.as_ref())
                        {
                            n.state = State::Cancelled;
                            n.detail = "cancelled by host".into();
                        }
                        Outcome::Applied(s)
                    }
                }
            }
        } else {
            Outcome::Failed(if NAMES[self.fixture % NAMES.len()] == "long-error" {
                format!(
                    "{} END OF FULL ERROR",
                    "host permission explanation ".repeat(80)
                )
            } else {
                "permission denied; host refused request".into()
            })
        };
        self.log = format!("host result accepted: {}", self.tree.resolve(&r, outcome));
        self.drain();
    }
    fn stale(&mut self) {
        if let Some(r) = &self.last {
            self.log = format!(
                "stale result rejected: {}",
                !self.tree.resolve(r, Outcome::Opened)
            );
        }
    }
    fn refresh(&mut self, material: bool) {
        let mut s = self.tree.snapshot().clone();
        s.revision += 1;
        if material {
            if let Some(Cursor::Node(id)) = self.tree.cursor()
                && let Some(n) = s.nodes.iter_mut().find(|n| n.id == *id)
            {
                n.revision += 1;
                n.name.push_str(" updated by host");
            }
        } else {
            s.nodes
                .insert(3, Node::new("quiet", "quiet-hours.ts", Some("digest")));
        }
        let _ = self.tree.replace(s);
    }
    fn remove(&mut self) {
        let Some(Cursor::Node(id)) = self.tree.cursor().cloned() else {
            return;
        };
        let mut s = self.tree.snapshot().clone();
        let mut removed = std::collections::HashSet::from([id]);
        loop {
            let old = removed.len();
            for n in &s.nodes {
                if n.parent.as_ref().is_some_and(|p| removed.contains(p)) {
                    removed.insert(n.id.clone());
                }
            }
            if old == removed.len() {
                break;
            }
        }
        s.nodes.retain(|n| !removed.contains(&n.id));
        s.revision += 1;
        let _ = self.tree.replace(s);
    }
    fn open_palette(&mut self) {
        self.palette = ChoicePicker::new(
            vec![Choice::new("files", "Files tree")],
            None,
            SelectionMode::Single,
            Filter::Off,
        )
        .ok();
    }
    fn reopen(&mut self) {
        let snapshot = if self.tree.snapshot().title == "tasks" {
            snapshot(false)
        } else {
            self.tree.snapshot().clone()
        };
        self.tree = TreeBrowser::new(snapshot, Placement::FullScreen)
            .expect("retained normalized snapshot");
        self.showing = true;
        self.panel = false;
    }
    pub(super) fn update(&mut self, event: &Event) -> bool {
        if let Some(palette) = &mut self.palette {
            match palette.update(event) {
                Pick::Selected(_) => {
                    self.palette = None;
                    self.chat.request_files();
                }
                Pick::Cancelled => self.palette = None,
                _ => {}
            }
            self.chat_intents();
            return true;
        }
        if let Event::Key(k) = event
            && k.kind == KeyEventKind::Press
            && k.modifiers.is_empty()
        {
            match k.code {
                KeyCode::F(2) => {
                    *self = Self::new((self.fixture + 1) % NAMES.len());
                    return true;
                }
                KeyCode::F(5) => {
                    self.reply(true);
                    return true;
                }
                KeyCode::F(6 | 7) => {
                    self.reply(false);
                    return true;
                }
                KeyCode::F(8) => {
                    self.refresh(true);
                    self.drain();
                    return true;
                }
                KeyCode::F(9) => {
                    self.remove();
                    self.drain();
                    return true;
                }
                KeyCode::F(12) => {
                    self.stale();
                    return true;
                }
                KeyCode::F(1) => {
                    self.open_palette();
                    return true;
                }
                KeyCode::Tab if self.showing && !self.tree.focused() => {
                    self.tree.set_focused(true);
                    if let Some(viewer) = &mut self.file_inspection {
                        viewer.close();
                    }
                    return true;
                }
                _ => {}
            }
        }
        if self.showing && self.tree.focused() {
            let consumed = self.tree.update(event);
            self.drain();
            consumed
        } else {
            if matches!(event,Event::Key(k) if matches!(k.code,KeyCode::F(10) | KeyCode::Char('c' | 't')) && (k.code==KeyCode::F(10)||k.modifiers.contains(KeyModifiers::CONTROL)))
            {
                return false;
            }
            if self.showing {
                self.update_inspection(event);
            } else {
                self.chat.update(event);
            }
            self.chat_intents();
            true
        }
    }
    fn update_inspection(&mut self, event: &Event) {
        if let Some(viewer) = &mut self.file_inspection {
            viewer.update(event);
            for intent in viewer.take_intents() {
                if let crate::inspection::Intent::Request(r) = intent {
                    let outcome = match r.operation {
                        crate::inspection::Operation::Copy { .. } => {
                            crate::inspection::Outcome::Copied
                        }
                        crate::inspection::Operation::Editor { .. } => {
                            crate::inspection::Outcome::Opened
                        }
                        crate::inspection::Operation::Load => crate::inspection::Outcome::Loaded(
                            crate::inspection::State::Ready(super::inspection::content()),
                        ),
                    };
                    let _ = viewer.complete(&r, outcome);
                }
            }
            if !viewer.active() {
                self.tree.set_focused(true);
            }
        } else {
            self.inspection.update(event);
        }
    }
    fn chat_intents(&mut self) {
        let mut intents = self.chat.take_intents();
        intents.extend(self.inspection.take_intents());
        for intent in intents {
            if matches!(intent, ChatIntent::Files(_)) {
                self.reopen();
            }
        }
    }
    pub(super) fn tick(&mut self, elapsed: Duration) {
        self.tree.tick(elapsed);
        self.elapsed = self.elapsed.saturating_add(elapsed);
        if self.delayed && self.elapsed >= Duration::from_secs(2) {
            self.delayed = false;
            self.reply(true);
        }
    }
    fn render_inspection(&mut self, frame: &mut Frame<'_>, area: Rect, p: Presentation) {
        if let Some(viewer) = &mut self.file_inspection {
            viewer.set_focused(!self.tree.focused());
            viewer.render(frame, area, p);
            return;
        }
        if !self.tree.focused() {
            self.inspection.render_docked_with(frame, area, p);
            return;
        }
        // An in-memory preview cannot claim the real frame's keyboard cursor.
        // It owns no terminal lease or I/O; the gallery owns the physical terminal.
        let area = area.intersection(frame.area());
        if area.is_empty() {
            return;
        }
        let Ok(mut preview) = Terminal::new(TestBackend::new(area.width, area.height));
        let Ok(_) = preview.draw(|f| self.inspection.render_docked_with(f, f.area(), p));
        let source = preview.backend().buffer();
        let target = frame.buffer_mut();
        for y in 0..area.height {
            for x in 0..area.width {
                target[(area.x + x, area.y + y)].clone_from(&source[(x, y)]);
            }
        }
    }
    pub(super) fn render(&mut self, frame: &mut Frame<'_>, area: Rect, p: Presentation) {
        let area = if NAMES[self.fixture % NAMES.len()] == "tiny" {
            Rect::new(area.x, area.y, area.width.min(20), area.height.min(3))
        } else {
            area
        };
        if !self.showing {
            self.chat.render_docked_with(frame, area, p);
        } else if self.panel && area.width >= 60 {
            let width = if area.width >= 100 { 34 } else { 24 };
            self.tree
                .render(frame, Rect::new(area.x, area.y, width, area.height), p);
            self.render_inspection(
                frame,
                Rect::new(
                    area.x + width + 1,
                    area.y,
                    area.width - width - 1,
                    area.height,
                ),
                p,
            );
        } else if self.tree.focused() {
            self.tree.render(frame, area, p);
        } else {
            self.render_inspection(frame, area, p);
        }
        if !self.log.is_empty() && area.height > 4 {
            frame.render_widget(
                Paragraph::new(self.log.clone()).style(p.muted()),
                Rect::new(area.x, area.y + area.height - 2, area.width, 1),
            );
        }
        if let Some(palette) = &mut self.palette {
            palette.render(
                frame,
                area,
                PickerView {
                    title: "commands",
                    presentation: p,
                    ..PickerView::default()
                },
                true,
            );
        }
    }
}
