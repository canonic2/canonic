//! Parses bounded semantic blocks; decorations never enter copy payloads.

use super::model::{Content, target};
use pulldown_cmark::{CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use std::fmt::Write;
use std::ops::Range;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "Independent combinable text attributes, not mutually exclusive workflow states"
)]
pub(super) struct Ink {
    pub bold: bool,
    pub italic: bool,
    pub muted: bool,
    pub accent: bool,
    pub link: bool,
    pub info: bool,
    pub surface: bool,
    pub error: bool,
    pub success: bool,
    pub warning: bool,
}
#[derive(Clone, Debug)]
pub(super) struct Run {
    pub text: String,
    pub ink: Ink,
    pub generated: bool,
}
#[derive(Clone, Debug)]
pub(super) enum Kind {
    Tool {
        id: String,
    },
    ToolGroup,
    Text,
    Quote,
    Rule,
    Code {
        language: String,
        file: String,
        line: Option<u32>,
    },
    Table {
        headers: Vec<String>,
        rows: Vec<Vec<String>>,
    },
    Diff {
        file: String,
        summary: String,
    },
    Fold {
        summary: String,
        reasoning: bool,
    },
}
#[derive(Clone, Debug)]
pub(super) struct Block {
    pub start: usize,
    pub markdown: String,
    pub runs: Vec<Run>,
    pub kind: Kind,
    pub open: Option<String>,
}
impl Block {
    pub(super) fn semantic(&self) -> String {
        self.runs
            .iter()
            .filter(|r| !r.generated)
            .map(|r| r.text.as_str())
            .collect()
    }
    fn plain(text: String, kind: Kind, ink: Ink) -> Self {
        Self {
            start: 0,
            markdown: text.clone(),
            runs: vec![Run {
                text,
                ink,
                generated: false,
            }],
            kind,
            open: None,
        }
    }
    pub(super) fn code(&self) -> Option<String> {
        if matches!(self.kind, Kind::Tool { .. }) {
            return self
                .markdown
                .split_once('\n')
                .map(|(_, output)| output.to_owned());
        }
        matches!(self.kind, Kind::Code { .. }).then(|| self.semantic())
    }
}

pub(super) fn blocks(content: &Content, budget: usize) -> Vec<Block> {
    if budget == 0 {
        return vec![omitted(0)];
    }
    let muted = Ink {
        muted: true,
        ..Ink::default()
    };
    match content {
        Content::ToolCard(call) => vec![Block::plain(
            call.copy_text(),
            Kind::Tool {
                id: call.id.clone(),
            },
            muted,
        )],
        Content::ToolGroup(group) => tool_group_blocks(group, muted),
        Content::Markdown(text) => markdown(text, budget),
        Content::Code {
            text,
            language,
            file,
            line,
        } => vec![code(text, language, file, *line)],
        Content::Table { headers, rows } => vec![table(headers.clone(), rows.clone(), None)],
        Content::Diff {
            file,
            patch,
            summary,
        } => vec![Block::plain(
            patch.clone(),
            Kind::Diff {
                file: file.clone(),
                summary: summary.clone(),
            },
            Ink::default(),
        )],
        Content::Reasoning { summary, text } => vec![Block::plain(
            text.clone(),
            Kind::Fold {
                summary: summary.clone(),
                reasoning: true,
            },
            Ink {
                italic: true,
                muted: true,
                ..Ink::default()
            },
        )],
        Content::Tool {
            verb,
            target,
            summary,
            output,
            open,
        } => {
            let heading = format!(
                "{verb} {target}{}",
                if summary.is_empty() {
                    String::new()
                } else {
                    format!(" · {summary}")
                }
            );
            let mut block = Block::plain(
                if output.is_empty() {
                    heading.clone()
                } else {
                    format!("{heading}\n{output}")
                },
                if output.is_empty() {
                    Kind::Text
                } else {
                    Kind::Fold {
                        summary: heading,
                        reasoning: false,
                    }
                },
                muted,
            );
            block.open.clone_from(open);
            if output.is_empty() {
                block.runs = tool_runs(verb, target, summary, open.is_some());
            }
            vec![block]
        }
        Content::Attachment {
            name,
            size,
            target,
            unavailable,
        } => vec![attachment(name, size, target.as_deref(), unavailable)],
        Content::Status(text) => vec![Block::plain(text.clone(), Kind::Text, muted)],
        Content::Unknown { kind, description } => vec![Block::plain(
            format!("[{kind} · {description}]"),
            Kind::Text,
            muted,
        )],
    }
}
fn tool_group_blocks(group: &crate::tool_call_card::ToolGroup, muted: Ink) -> Vec<Block> {
    let mut blocks = vec![Block::plain(
        format!("{} at once", group.calls.len()),
        Kind::ToolGroup,
        muted,
    )];
    let mut start = blocks[0].semantic().len() + 1;
    for call in &group.calls {
        let mut block = Block::plain(
            call.copy_text(),
            Kind::Tool {
                id: call.id.clone(),
            },
            muted,
        );
        block.start = start;
        start += block.semantic().len() + 1;
        blocks.push(block);
    }
    blocks
}

fn attachment(name: &str, size: &str, target: Option<&str>, unavailable: &str) -> Block {
    let text = if target.is_some() {
        format!("▤ {name} · {size} ↗")
    } else {
        format!(
            "▤ {name} · {}",
            if unavailable.is_empty() {
                "unavailable"
            } else {
                unavailable
            }
        )
    };
    let mut block = Block::plain(
        text,
        Kind::Text,
        Ink {
            link: target.is_some(),
            muted: target.is_none(),
            ..Ink::default()
        },
    );
    block.open = target.map(str::to_owned);
    block
}

fn tool_runs(verb: &str, target: &str, summary: &str, open: bool) -> Vec<Run> {
    let muted = Ink {
        muted: true,
        ..Ink::default()
    };
    let mut runs = vec![
        Run {
            text: verb.into(),
            ink: muted,
            generated: false,
        },
        Run {
            text: " ".repeat(8usize.saturating_sub(verb.len())),
            ink: muted,
            generated: true,
        },
        Run {
            text: " ".into(),
            ink: muted,
            generated: false,
        },
        Run {
            text: target.into(),
            ink: Ink {
                info: true,
                ..Ink::default()
            },
            generated: false,
        },
    ];
    if open {
        runs.push(Run {
            text: " ↗".into(),
            ink: muted,
            generated: true,
        });
    }
    if !summary.is_empty() {
        runs.push(Run {
            text: format!(" · {summary}"),
            ink: muted,
            generated: false,
        });
    }
    runs
}

fn table(headers: Vec<String>, rows: Vec<Vec<String>>, source: Option<&str>) -> Block {
    let markdown = source.map_or_else(
        || {
            let mut out = format!(
                "| {} |\n| {} |",
                headers.join(" | "),
                vec!["---"; headers.len()].join(" | ")
            );
            for row in &rows {
                let _ = write!(out, "\n| {} |", row.join(" | "));
            }
            out
        },
        str::to_owned,
    );
    let mut semantic = headers.join(" · ");
    for row in &rows {
        semantic.push('\n');
        semantic.push_str(&row.join(" · "));
    }
    Block {
        start: 0,
        markdown,
        runs: vec![Run {
            generated: false,
            text: semantic,
            ink: Ink::default(),
        }],
        kind: Kind::Table { headers, rows },
        open: None,
    }
}

#[derive(Default)]
struct Markdown {
    result: Vec<Block>,
    current: Option<Block>,
    depth: usize,
    end: usize,
    styles: Vec<Ink>,
    lists: Vec<Option<u64>>,
    row: Vec<String>,
    headers: Vec<String>,
    rows: Vec<Vec<String>>,
    cell: String,
    in_table: bool,
    in_head: bool,
}

fn omitted(start: usize) -> Block {
    let mut block = Block::plain(
        "⋯ content not drawn · block limit".into(),
        Kind::Text,
        Ink {
            muted: true,
            ..Ink::default()
        },
    );
    block.start = start;
    block.runs[0].generated = true;
    block
}
fn markdown(source: &str, budget: usize) -> Vec<Block> {
    if source.is_empty() {
        return vec![Block::plain(String::new(), Kind::Text, Ink::default())];
    }
    let mut state = Markdown::default();
    for (event, range) in
        Parser::new_ext(source, Options::ENABLE_TABLES | Options::ENABLE_TASKLISTS)
            .into_offset_iter()
    {
        state.event(event, range, source);
        if state.result.len() >= budget {
            break;
        }
    }
    state.flush(source);
    if state.result.len() >= budget && state.end < source.len() {
        state.result.push(omitted(state.end));
    }
    state.result
}

impl Markdown {
    fn ink(&self) -> Ink {
        self.styles.last().copied().unwrap_or_default()
    }
    fn add(&mut self, text: &str, ink: Ink) {
        self.add_run(text, ink, false);
    }
    fn decorate(&mut self, text: &str, ink: Ink) {
        self.add_run(text, ink, true);
    }
    fn add_run(&mut self, text: &str, ink: Ink, generated: bool) {
        if self.in_table {
            self.cell.push_str(text);
            return;
        }
        if let Some(block) = &mut self.current {
            if let Some(last) = block
                .runs
                .last_mut()
                .filter(|r| r.ink == ink && r.generated == generated)
            {
                last.text.push_str(text);
            } else {
                block.runs.push(Run {
                    generated,
                    text: text.into(),
                    ink,
                });
            }
        }
    }
    fn newline(&mut self) {
        if self
            .current
            .as_ref()
            .is_some_and(|b| b.runs.last().is_some_and(|r| !r.text.ends_with('\n')))
        {
            self.add("\n", self.ink());
        }
    }
    fn flush(&mut self, source: &str) {
        if let Some(mut block) = self.current.take() {
            block.markdown = source
                .get(block.start..self.end)
                .unwrap_or("")
                .trim_end_matches('\n')
                .into();
            if matches!(block.kind, Kind::Table { .. }) {
                let mut parsed = table(
                    std::mem::take(&mut self.headers),
                    std::mem::take(&mut self.rows),
                    Some(&block.markdown),
                );
                parsed.start = block.start;
                block = parsed;
            }
            if let Kind::Code { language, .. } = &block.kind {
                block.runs = super::syntax::highlight(&block.semantic(), language);
            }
            self.result.push(block);
        }
        self.styles.clear();
        self.in_table = false;
    }
    fn start(&mut self, tag: &Tag<'_>, range: &Range<usize>) {
        if self.depth == 0 {
            let kind = match tag {
                Tag::CodeBlock(CodeBlockKind::Fenced(info)) => Kind::Code {
                    language: info.split_whitespace().next().unwrap_or("").into(),
                    file: String::new(),
                    line: None,
                },
                Tag::CodeBlock(_) => Kind::Code {
                    language: String::new(),
                    file: String::new(),
                    line: None,
                },
                Tag::BlockQuote(_) => Kind::Quote,
                Tag::Table(_) => Kind::Table {
                    headers: vec![],
                    rows: vec![],
                },
                _ => Kind::Text,
            };
            self.current = Some(Block {
                start: range.start,
                markdown: String::new(),
                runs: vec![],
                kind,
                open: None,
            });
            self.end = range.end;
        }
        self.depth += 1;
        let mut ink = self.ink();
        match tag {
            Tag::Heading { level, .. } => {
                ink.bold = true;
                ink.accent = matches!(level, HeadingLevel::H1 | HeadingLevel::H2);
            }
            Tag::Emphasis => ink.italic = true,
            Tag::Strong => ink.bold = true,
            Tag::BlockQuote(_) | Tag::HtmlBlock => ink.muted = true,
            Tag::Link { dest_url, .. } | Tag::Image { dest_url, .. } => {
                ink.link = true;
                if let Some(block) = &mut self.current
                    && block.open.is_none()
                {
                    block.open = target(dest_url);
                }
            }
            Tag::List(number) => self.lists.push(*number),
            Tag::Item => {
                self.newline();
                let indent = "  ".repeat(self.lists.len().saturating_sub(1).min(32));
                let number = self.lists.last_mut().and_then(Option::as_mut);
                let (prefix, ordered) = number.map_or_else(
                    || ("· ".into(), false),
                    |n| {
                        let p = format!("{n}. ");
                        *n = n.saturating_add(1);
                        (p, true)
                    },
                );
                self.decorate(
                    &format!("{indent}{prefix}"),
                    Ink {
                        muted: ordered,
                        accent: !ordered,
                        ..Ink::default()
                    },
                );
            }
            Tag::Table(_) => self.in_table = true,
            Tag::TableHead => {
                self.in_head = true;
                self.row.clear();
            }
            Tag::TableRow => self.row.clear(),
            Tag::TableCell => self.cell.clear(),
            _ => {}
        }
        self.styles.push(ink);
    }
    fn finish(&mut self, tag: TagEnd, source: &str) {
        match tag {
            TagEnd::Link | TagEnd::Image => self.decorate(" ↗", self.ink()),
            TagEnd::List(_) => {
                self.lists.pop();
            }
            TagEnd::Paragraph | TagEnd::Item => self.newline(),
            TagEnd::TableCell => self.row.push(std::mem::take(&mut self.cell)),
            TagEnd::TableHead => {
                self.headers = std::mem::take(&mut self.row);
                self.in_head = false;
            }
            TagEnd::TableRow if !self.in_head => self.rows.push(std::mem::take(&mut self.row)),
            _ => {}
        }
        self.depth = self.depth.saturating_sub(1);
        self.styles.pop();
        if self.depth == 0 {
            self.flush(source);
        }
    }
    fn event(&mut self, event: Event<'_>, range: Range<usize>, source: &str) {
        self.end = self.end.max(range.end);
        match event {
            Event::Start(tag) => self.start(&tag, &range),
            Event::End(tag) => self.finish(tag, source),
            Event::Text(text) => self.add(&text, self.ink()),
            Event::Code(text) => self.add(
                &text,
                Ink {
                    surface: true,
                    ..self.ink()
                },
            ),
            Event::Html(text) | Event::InlineHtml(text) => self.add(
                &text,
                Ink {
                    muted: true,
                    ..self.ink()
                },
            ),
            Event::SoftBreak => self.add(" ", self.ink()),
            Event::HardBreak => self.add("\n", self.ink()),
            Event::TaskListMarker(done) => self.add(if done { "[x] " } else { "[ ] " }, self.ink()),
            Event::Rule => {
                self.flush(source);
                let mut block = Block::plain(
                    String::new(),
                    Kind::Rule,
                    Ink {
                        muted: true,
                        ..Ink::default()
                    },
                );
                block.start = range.start;
                block.markdown = source[range].into();
                self.result.push(block);
            }
            _ => {}
        }
    }
}

fn code(text: &str, language: &str, file: &str, line: Option<u32>) -> Block {
    let mut block = Block::plain(
        text.into(),
        Kind::Code {
            language: language.into(),
            file: file.into(),
            line,
        },
        Ink::default(),
    );
    block.markdown = format!(
        "```{language}\n{text}{}```",
        if text.ends_with('\n') { "" } else { "\n" }
    );
    block.open =
        target(file).map(|file| line.map_or(file.clone(), |line| format!("{file}:{line}")));
    block.runs = super::syntax::highlight(text, language);
    block
}
