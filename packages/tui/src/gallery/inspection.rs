//! Deterministic inspection host and retained Chat/tree composition; no external I/O.

use crate::{
    chat::{Chat, Content as ChatContent, Intent as ChatIntent, Part, Role, Turn},
    event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers},
    inspection::{Content, Inspector, Item, Operation, Origin, Outcome, Request, State},
    theme::Presentation,
    tree_browser::{
        Action, Intent as TreeIntent, Node, Outcome as TreeOutcome, Placement, Snapshot,
        TreeBrowser,
    },
};
use ratatui::{Frame, layout::Rect};

pub(super) const NAMES: &[&str] = &[
    "main",
    "code",
    "diff",
    "empty",
    "loading",
    "load-error",
    "retrying",
    "binary",
    "generated",
    "too-large",
    "partial",
    "deleted",
    "renamed",
    "long",
    "folded",
    "unfolded",
    "next-hunk",
    "last-hunk",
    "go-to-line",
    "whole-file",
    "wrapped",
    "find",
    "next-match",
    "no-match",
    "smart-case",
    "selection",
    "copied-code",
    "copied-patch",
    "copy-failed",
    "editor-pending",
    "editor-failed",
    "stale-result",
    "foreign-result",
    "duplicate-result",
    "from-chat",
    "return-chat",
    "from-tree",
    "return-tree",
    "opening",
    "refused",
    "late-result",
    "reopen",
    "narrow",
    "narrow-code",
    "tiny",
    "ansi16",
    "no-color",
    "ascii",
    "light",
    "unicode",
    "controls",
];
pub(super) const PATCH: &str = "@@ -9,8 +9,9 @@ digestHeaders\n export function digestHeaders(user: User) {\n   return {\n     headers: {\n-      json,\n+      json,\n+      'X-Timezone': user.timezone,\n     },\n   }\n }\n \n@@ -18,3 +19,4 @@ nextSend\n export function nextSend(user: User) {\n-  return atHour(s.hour, 0)\n+  const tz = user.timezone ?? 'UTC'\n+  return atHour(s.hour, offsetFor(tz))\n }\n";
fn file() -> String {
    let mut lines = vec!["// Acme digest settings".to_owned(); 8];
    lines.extend(
        [
            "export function digestHeaders(user: User) {",
            "  return {",
            "    headers: {",
            "      json,",
            "      'X-Timezone': user.timezone,",
            "    },",
            "  }",
            "}",
            "",
            "// next scheduled send",
            "export function nextSend(user: User) {",
            "  const tz = user.timezone ?? 'UTC'",
            "  return atHour(s.hour, offsetFor(tz))",
            "}",
        ]
        .map(str::to_owned),
    );
    lines.join("\n")
}
pub(super) fn content() -> Content {
    Content::Diff {
        patch: PATCH.into(),
        new_file: Some(file()),
    }
}
pub(super) fn viewer(id: &str, origin: Origin) -> Inspector {
    let mut item = Item::new(id, "src/digest/settings.ts");
    item.origin = origin;
    item.editor = true;
    item.summary = "M +3 -1".into();
    item.language = "TypeScript".into();
    item.line = Some(13);
    Inspector::new(item, State::Ready(content())).expect("valid inspection fixture")
}
pub(super) fn from_chat(
    id: &str,
    content: ChatContent,
) -> Result<Inspector, crate::inspection::Invalid> {
    let mut item = Item::new(id, "Full loaded content");
    let source = match content {
        ChatContent::Diff {
            file,
            patch,
            summary,
        } => {
            if !file.is_empty() {
                item.title = file;
            }
            item.summary = summary;
            Content::Diff {
                patch,
                new_file: None,
            }
        }
        ChatContent::Code {
            text,
            file,
            language,
            line,
        } => {
            if !file.is_empty() {
                item.title = file;
            }
            item.language = language;
            item.line = line.and_then(|n| usize::try_from(n).ok());
            item.summary = "supplied source".into();
            Content::CodeAt {
                text,
                first_line: item.line.unwrap_or(1).max(1),
            }
        }
        ChatContent::Table { headers, rows } => Content::Code(format!(
            "{}\n{}",
            headers.join(" | "),
            rows.iter()
                .map(|r| r.join(" | "))
                .collect::<Vec<_>>()
                .join("\n")
        )),
        ChatContent::Markdown(text) => Content::Code(text),
        _ => Content::Code("unsupported inspection content".into()),
    };
    Inspector::new(item, State::Ready(source))
}
fn key(code: KeyCode) -> Event {
    Event::Key(KeyEvent::new(code, KeyModifiers::NONE))
}
pub(super) struct InspectionDemo {
    viewer: Inspector,
    chat: Chat,
    tree: TreeBrowser,
    fixture: usize,
    tree_origin: bool,
    old: Option<Request>,
    log: String,
}
impl InspectionDemo {
    #[expect(
        clippy::too_many_lines,
        reason = "Each named design fixture configures one deterministic observable state."
    )]
    pub(super) fn new(fixture: usize) -> Self {
        let name = NAMES[fixture % NAMES.len()];
        let mut item = Item::new("settings", "src/digest/settings.ts");
        item.editor = true;
        item.summary = "M +3 -1".into();
        item.language = "TypeScript".into();
        let data=match name {
            "code"|"narrow-code"=>Content::Code(file()),"empty"=>Content::Code(String::new()),
            "long"|"wrapped"=>Content::Code((1..=4012).map(|i|format!("const zone_{i} = {{ name: 'Pacific/Kiritimati', offset: '+14:00', digest: 'timezone scheduled digest' }};")).collect::<Vec<_>>().join("\n")),
            "deleted"=>Content::Diff {patch:"@@ -1,3 +0,0 @@ deleted offsets\n-// removed offset table\n-export const OFFSETS = {}\n-// use named timezones\n".into(),new_file:None},
            "folded"|"unfolded"=>Content::Diff {patch:format!("@@ -1,20 +1,20 @@ unchanged context\n{}",(1..=20).map(|i|format!(" line-{i} timezone")).collect::<Vec<_>>().join("\n")),new_file:None},
            "unicode"=>Content::Code("const city = '東京 👩‍💻 e\u{301}';\nconst İSTANBUL = 'İstanbul';".into()),
            "controls"=>Content::Code("safe\x1b[31mdata\x07\n tab\tvalue".into()),_=>content(),
        };
        let state = match name {
            "loading" | "opening" => State::Loading("waiting for the host".into()),
            "load-error" | "retrying" => State::Failed("permission denied".into()),
            "binary" => State::Unavailable {
                reason: "Binary file · 212 kB -> 198 kB · cannot display as text".into(),
                preview: None,
            },
            "generated" => State::Unavailable {
                reason: "Generated file · host offers editor only".into(),
                preview: None,
            },
            "too-large" | "partial" => State::Unavailable {
                reason: "4.2 MB exceeds host limit · first 20 lines available".into(),
                preview: Some(data.clone()),
            },
            _ => State::Ready(data),
        };
        if name == "renamed" {
            item.summary = "R from fmt.ts".into();
        }
        let tree_origin = matches!(name, "from-tree" | "return-tree");
        if tree_origin {
            item.origin = Origin::Tree;
        }
        let mut chat = Chat::new();
        chat.set_input("and bump the app version");
        chat.update(&key(KeyCode::Left));
        let mut turn = Turn::new("digest", Role::Assistant, "acme", "");
        turn.parts = vec![Part::new(
            "patch",
            ChatContent::Diff {
                file: "src/digest/settings.ts".into(),
                patch: PATCH.into(),
                summary: "M +3 -1".into(),
            },
        )];
        chat.push_record(turn);
        let mut node = Node::new("settings", "settings.ts", None);
        let mut action = Action::new("open", "inspect");
        action.opens = true;
        node.actions.push(action);
        node.primary = Some("open".into());
        let tree = TreeBrowser::new(
            Snapshot {
                title: "files".into(),
                nodes: vec![node],
                ..Snapshot::default()
            },
            Placement::Panel,
        )
        .expect("valid fixture");
        let mut demo = Self {
            viewer: Inspector::new(item, state).expect("valid fixture"),
            chat,
            tree,
            fixture,
            tree_origin,
            old: None,
            log: String::new(),
        };
        match name {
            "partial" => {
                demo.viewer.update(&key(KeyCode::Enter));
            }
            "retrying" => {
                demo.viewer.update(&key(KeyCode::Char('r')));
            }
            "unfolded" => {
                for _ in 0..4 {
                    demo.viewer.update(&key(KeyCode::Down));
                }
                demo.viewer.update(&key(KeyCode::Enter));
            }
            "next-hunk" => {
                demo.viewer.update(&key(KeyCode::Char(']')));
                demo.viewer.update(&key(KeyCode::Char(']')));
            }
            "last-hunk" => {
                demo.viewer.update(&key(KeyCode::End));
                demo.viewer.update(&key(KeyCode::Char(']')));
            }
            "go-to-line" => {
                demo.viewer.update(&key(KeyCode::Char(':')));
                demo.viewer.update(&Event::Paste("19".into()));
                demo.viewer.update(&key(KeyCode::Enter));
            }
            "whole-file" => {
                demo.viewer.update(&key(KeyCode::Tab));
            }
            "wrapped" => {
                demo.viewer.update(&key(KeyCode::Char('w')));
            }
            "find" | "next-match" | "no-match" | "smart-case" => {
                demo.viewer.update(&key(KeyCode::Char('/')));
                demo.viewer.update(&Event::Paste(
                    if name == "no-match" {
                        "tzz"
                    } else if name == "smart-case" {
                        "Timezone"
                    } else {
                        "tz"
                    }
                    .into(),
                ));
                if name == "next-match" {
                    demo.viewer.update(&key(KeyCode::Enter));
                    demo.viewer.update(&key(KeyCode::Char('n')));
                }
            }
            "selection" | "copied-code" | "copied-patch" | "copy-failed" | "stale-result"
            | "foreign-result" | "duplicate-result" => {
                for _ in 0..4 {
                    demo.viewer.update(&key(KeyCode::Down));
                }
                demo.viewer.update(&key(KeyCode::Char('v')));
                demo.viewer.update(&key(KeyCode::Down));
                demo.viewer.update(&key(KeyCode::Down));
                if name != "selection" {
                    demo.viewer
                        .update(&key(KeyCode::Char(if name == "copied-code" {
                            'c'
                        } else {
                            'y'
                        })));
                }
                if matches!(name, "copied-code" | "copied-patch") {
                    demo.reply(true);
                }
                if name == "copy-failed" {
                    demo.reply(false);
                }
                if name == "stale-result" {
                    demo.old = demo.viewer.pending().cloned();
                    demo.viewer.close();
                    demo.viewer.reopen(Origin::Chat);
                    demo.stale();
                }
                if name == "foreign-result" {
                    let mut foreign = viewer("settings", Origin::Chat);
                    foreign.update(&key(KeyCode::Char('v')));
                    foreign.update(&key(KeyCode::Char('y')));
                    demo.old = foreign.pending().cloned();
                    demo.stale();
                }
                if name == "duplicate-result" {
                    demo.old = demo.viewer.pending().cloned();
                    demo.reply(true);
                    demo.stale();
                }
            }
            "editor-pending" | "editor-failed" => {
                demo.viewer.update(&key(KeyCode::Char('e')));
                if name == "editor-failed" {
                    demo.reply(false);
                }
            }
            "from-chat" | "return-chat" | "refused" => {
                demo.viewer.close();
                demo.viewer.take_intents();
                demo.chat.update(&Event::Key(KeyEvent::new(
                    KeyCode::Char('x'),
                    KeyModifiers::CONTROL,
                )));
                demo.chat.update(&key(KeyCode::Char('v')));
                demo.chat.update(&key(KeyCode::Char('o')));
                demo.chat_intents(name == "refused");
                if name == "return-chat" {
                    demo.viewer.close();
                }
            }
            "return-tree" => demo.viewer.close(),
            "late-result" => {
                demo.viewer
                    .replace(demo.new_item(), State::Failed("load failure".into()))
                    .expect("revision");
                demo.viewer.update(&key(KeyCode::Char('r')));
                demo.old = demo.viewer.pending().cloned();
                demo.viewer.close();
                demo.stale();
            }
            "reopen" => {
                demo.viewer.goto(19);
                demo.viewer.close();
                demo.viewer.reopen(Origin::Chat);
            }
            _ => {}
        }
        demo
    }
    fn new_item(&self) -> Item {
        let mut item = self.viewer.item().clone();
        item.revision += 1;
        item
    }
    fn reply(&mut self, success: bool) {
        let Some(r) = self.viewer.pending().cloned() else {
            return;
        };
        self.old = Some(r.clone());
        let result = if success {
            match r.operation {
                Operation::Load => Outcome::Loaded(State::Ready(content())),
                Operation::Copy { .. } => Outcome::Copied,
                Operation::Editor { .. } => Outcome::Opened,
            }
        } else {
            Outcome::Failed("no clipboard/editor in this session".into())
        };
        self.viewer.complete(&r, result).expect("valid result");
        self.viewer.take_intents();
    }
    fn stale(&mut self) {
        if let Some(r) = &self.old {
            let rejected = !self
                .viewer
                .complete(r, Outcome::Copied)
                .expect("valid stale result");
            self.log = format!("stale/foreign/duplicate result rejected: {rejected}");
        }
    }
    fn chat_intents(&mut self, refuse: bool) {
        for intent in self.chat.take_intents() {
            if let ChatIntent::Inspect { request, .. } = intent {
                if refuse {
                    self.chat
                        .acknowledge(&request, Err("the patch is no longer available"));
                } else {
                    self.chat.acknowledge(&request, Ok(""));
                    self.viewer.reopen(Origin::Chat);
                }
            }
        }
    }
    pub(super) fn update(&mut self, event: &Event) -> bool {
        if let Event::Key(k) = event
            && k.kind == KeyEventKind::Press
        {
            if k.code == KeyCode::F(5) {
                self.reply(true);
                return true;
            }
            if k.code == KeyCode::F(6) {
                self.reply(false);
                return true;
            }
            if k.code == KeyCode::F(12) {
                self.stale();
                return true;
            }
            if k.code == KeyCode::F(2) {
                *self = Self::new((self.fixture + 1) % NAMES.len());
                return true;
            }
            if k.code == KeyCode::F(10)
                || k.modifiers.contains(KeyModifiers::CONTROL)
                    && matches!(k.code, KeyCode::Char('c' | 't'))
            {
                return false;
            }
        }
        if self.viewer.active() {
            self.viewer.update(event);
        } else if self.tree_origin {
            self.tree.update(event);
            for intent in self.tree.take_intents() {
                if let TreeIntent::Request(r) = intent {
                    self.tree.resolve(&r, TreeOutcome::Opened);
                    self.viewer.reopen(Origin::Tree);
                }
            }
        } else {
            self.chat.update(event);
            self.chat_intents(false);
        }
        true
    }
    pub(super) fn render(&mut self, f: &mut Frame<'_>, area: Rect, p: Presentation) {
        let area = if NAMES[self.fixture % NAMES.len()] == "tiny" {
            Rect::new(area.x, area.y, area.width.min(20), area.height.min(3))
        } else {
            area
        };
        if self.viewer.active() {
            if self.tree_origin && area.width >= 60 {
                let width = 24;
                self.tree.set_focused(false);
                self.tree
                    .render(f, Rect::new(area.x, area.y, width, area.height), p);
                self.viewer.render(
                    f,
                    Rect::new(
                        area.x + width + 1,
                        area.y,
                        area.width - width - 1,
                        area.height,
                    ),
                    p,
                );
            } else {
                self.viewer.render(f, area, p);
            }
        } else if self.tree_origin {
            self.tree.set_focused(true);
            self.tree.render(f, area, p);
        } else {
            self.chat.render_docked_with(f, area, p);
        }
        if !self.log.is_empty() && area.height > 4 {
            f.render_widget(
                ratatui::widgets::Paragraph::new(self.log.clone()).style(p.info()),
                Rect::new(area.x, area.bottom() - 2, area.width, 1),
            );
        }
    }
}
