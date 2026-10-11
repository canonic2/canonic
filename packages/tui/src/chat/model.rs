//! Owns identified transcript data and pure, bounded host contracts.

use std::sync::Arc;

/// Instance identity. Clones address this conversation; another Chat cannot accept it.
#[derive(Clone, Debug)]
pub struct Conversation(pub(super) Arc<()>);
impl PartialEq for Conversation {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}
impl Eq for Conversation {}

/// Opaque, single-use host request identity scoped to a Chat lifetime.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Request {
    pub(super) conversation: Conversation,
    pub(super) number: u64,
}

/// Semantic speaker. A note has no turn header.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    User,
    Assistant,
    Note,
}

/// Host-supplied outcome; stopped output is retained and is not a failure.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum End {
    /// Static host content without an asserted run outcome.
    Quiet,
    /// More identified updates can arrive.
    Live,
    /// Successful completion with optional display duration.
    Done(String),
    /// Cancellation with optional explanation.
    Stopped(String),
    /// Failure with explanation; retry is a new host reply.
    Failed(String),
}

/// One host turn with ordered, independently revised parts.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Turn {
    /// Stable host message ID, unique in the loaded projection.
    pub id: String,
    /// Speaker category.
    pub role: Role,
    /// Host display label (for example, acme).
    pub label: String,
    /// Host formatted clock, never read from the system.
    pub timestamp: String,
    /// Host day label; a change inserts a quiet date rule.
    pub day: String,
    /// Host supplied elapsed label once the turn ends; never a system clock read.
    pub duration: String,
    /// Host outcome.
    pub end: End,
    /// Parts in transcript order; IDs must be unique within this turn.
    pub parts: Vec<Part>,
}
impl Turn {
    /// Create a turn with one Markdown part named `body`.
    #[must_use]
    pub fn new(id: &str, role: Role, label: &str, markdown: &str) -> Self {
        Self {
            id: id.into(),
            role,
            label: label.into(),
            timestamp: String::new(),
            day: String::new(),
            duration: String::new(),
            end: End::Live,
            parts: vec![Part::new("body", Content::Markdown(markdown.into()))],
        }
    }
}

/// One identified content part. Revision zero is a valid initial snapshot.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Part {
    /// Stable identity within its turn.
    pub id: String,
    /// Monotonically increasing host revision.
    pub revision: u64,
    /// Safe display projection supplied by the host.
    pub content: Content,
}
impl Part {
    /// Create an initial part at revision zero.
    #[must_use]
    pub fn new(id: &str, content: Content) -> Self {
        Self {
            id: id.into(),
            revision: 0,
            content,
        }
    }
}

/// Content data, independent of transport, agent tools and terminal I/O.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Content {
    /// `CommonMark` with tables. Raw HTML and unsupported syntax stay readable.
    Markdown(String),
    /// Original source lines, optional language/file and starting line.
    Code {
        text: String,
        language: String,
        file: String,
        line: Option<u32>,
    },
    /// All columns are retained; narrow layouts become labeled records.
    Table {
        headers: Vec<String>,
        rows: Vec<Vec<String>>,
    },
    /// Host supplied unified patch. Chat previews the first hunk.
    Diff {
        file: String,
        patch: String,
        summary: String,
    },
    /// Generic activity projection, never an executable tool schema.
    Tool {
        verb: String,
        target: String,
        summary: String,
        output: String,
        open: Option<String>,
    },
    /// Only host supplied reasoning; folded by default.
    Reasoning { summary: String, text: String },
    /// Available document or explicitly unavailable attachment.
    Attachment {
        name: String,
        size: String,
        target: Option<String>,
        unavailable: String,
    },
    /// Quiet boundary or host status text.
    Status(String),
    /// A readable fallback for a kind this terminal cannot draw.
    Unknown { kind: String, description: String },
}

/// Identified part mutation; a revision mismatch never mutates loaded content.
#[derive(Clone, Debug)]
pub struct Update {
    /// Conversation token obtained from the target Chat.
    pub conversation: Conversation,
    /// Message and part identities.
    pub message: String,
    /// Part identity within the message.
    pub part: String,
    /// Revision this mutation expects.
    pub expected: u64,
    /// Next revision (must be expected + 1).
    pub revision: u64,
    /// Replacement or append operation.
    pub change: Change,
}
/// A part update cannot change a different part or fixed submitted turn.
#[derive(Clone, Debug)]
pub enum Change {
    /// Replace this part's content.
    Replace(Content),
    /// Append original text to Markdown, code or supplied reasoning.
    Append(String),
}

/// Result of an identified mutation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Updated {
    /// Exact target and next revision were accepted.
    Applied,
    /// Invalid identity/revision: ask the host for a fresh snapshot.
    Resync(Request),
    /// Another conversation's update was rejected without creating a request.
    Foreign,
}

/// Explicit host work. Chat never opens files, retries agents or accesses a clipboard.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Intent {
    /// Copy clean semantic content; acknowledge actual clipboard success/failure.
    Copy {
        request: Request,
        text: String,
        kind: String,
        lines: usize,
    },
    /// Open a sanitized URL/path supplied in content.
    Open { request: Request, target: String },
    /// Open the full loaded table/patch in the host's inspection view.
    Inspect {
        request: Request,
        message: String,
        part: String,
        content: Content,
    },
    /// Load messages before this ID, preserving the current logical anchor.
    Earlier { request: Request, before: String },
    /// Return to the host's newest window after an earlier window evicts newer turns.
    Latest { request: Request },
    /// Retry a failed reply; host adds a new reply and keeps the old one.
    Retry { request: Request, message: String },
}

/// Logical transcript position, independent of wrapped row numbers.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Anchor {
    /// Loaded message identity.
    pub message: String,
    /// Part identity; empty denotes the turn header.
    pub part: String,
    /// Semantic byte offset within the part.
    pub offset: usize,
}

/// Local focus owner. Composer input is untouched by Search and Select.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Compose,
    Search,
    Select,
}

/// Context supplied for an empty conversation, without owning a session browser.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Welcome {
    /// Product name.
    pub title: String,
    /// Model/project/actions context supplied by the host.
    pub context: String,
    /// Recent session summaries. Opening sessions belongs to the host.
    pub recent: Vec<String>,
    /// Suggested prompts.
    pub suggestions: Vec<String>,
}

/// Observable bounded-layout evidence, useful to a host and performance tests.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Metrics {
    /// Part parses since creation/reset.
    pub parses: usize,
    /// Wrapped part layouts since creation/reset.
    pub layouts: usize,
    /// Retained cached display rows (at most 32,768).
    pub cached_rows: usize,
    /// Estimated wrapped-row allocations, at most 8 MiB; source/parse storage is separate.
    pub cached_bytes: usize,
}

/// Convert terminal controls to visible text. LF/tab remain semantic source data.
pub(super) fn safe(text: &str) -> String {
    text.chars()
        .flat_map(|c| match c {
            '\u{1b}' => "␛".chars().collect::<Vec<_>>(),
            '\r' => "␍".chars().collect(),
            '\n' | '\t' => vec![c],
            c if c.is_control() => format!("[U+{:04X}]", u32::from(c)).chars().collect(),
            c => vec![c],
        })
        .collect()
}

pub(super) fn label(text: &str) -> String {
    crate::text::clean(&safe(text).replace(['\n', '\t'], " "), 256, false)
}

pub(super) fn target(text: &str) -> Option<String> {
    if text.is_empty()
        || text.len() > 4096
        || text.chars().any(char::is_control)
        || text.contains(['␛', '␍'])
        || text.contains("[U+")
    {
        return None;
    }
    let lower = text.trim().to_ascii_lowercase();
    if lower.contains(':')
        && !(lower.starts_with("https://")
            || lower.starts_with("http://")
            || lower.starts_with("mailto:")
            || lower.rsplit_once(':').is_some_and(|(path, n)| {
                (path.contains('/') || path.contains('.'))
                    && !path.contains(':')
                    && n.parse::<u32>().is_ok()
            }))
    {
        return None;
    }
    Some(text.trim().into())
}

impl Content {
    pub(super) fn bound_structure(&mut self, remaining: &mut usize) -> usize {
        let Self::Table { headers, rows } = self else {
            return 0;
        };
        let before = headers.len() + rows.iter().map(Vec::len).sum::<usize>();
        headers.truncate(64.min(*remaining));
        *remaining = remaining.saturating_sub(headers.len());
        let columns = headers
            .len()
            .max(rows.iter().map(Vec::len).max().unwrap_or(0).min(64))
            .max(1);
        rows.truncate(1024.min(*remaining / columns));
        for row in rows.iter_mut() {
            row.truncate(columns);
            *remaining = remaining.saturating_sub(row.len());
        }
        before.saturating_sub(headers.len() + rows.iter().map(Vec::len).sum::<usize>())
    }
    pub(super) fn text_mut(&mut self) -> Option<&mut String> {
        match self {
            Self::Markdown(t)
            | Self::Status(t)
            | Self::Code { text: t, .. }
            | Self::Reasoning { text: t, .. } => Some(t),
            _ => None,
        }
    }
    pub(super) fn sanitize(&mut self) {
        match self {
            Self::Markdown(t) | Self::Status(t) => *t = safe(t),
            Self::Code {
                text,
                language,
                file,
                ..
            } => {
                *text = safe(text);
                *language = label(language);
                *file = safe(file);
            }
            Self::Table { headers, rows } => {
                for cell in headers.iter_mut().chain(rows.iter_mut().flatten()) {
                    *cell = safe(cell);
                }
            }
            Self::Diff {
                file,
                patch,
                summary,
            } => {
                *file = label(file);
                *patch = safe(patch);
                *summary = label(summary);
            }
            Self::Tool {
                verb,
                target: name,
                summary,
                output,
                open,
            } => {
                *verb = label(verb);
                *name = label(name);
                *summary = label(summary);
                *output = safe(output);
                *open = open.as_deref().and_then(target);
            }
            Self::Reasoning { summary, text } => {
                *summary = label(summary);
                *text = safe(text);
            }
            Self::Attachment {
                name,
                size,
                target: path,
                unavailable,
            } => {
                *name = label(name);
                *size = label(size);
                *unavailable = label(unavailable);
                *path = path.as_deref().and_then(target);
            }
            Self::Unknown { kind, description } => {
                *kind = label(kind);
                *description = label(description);
            }
        }
    }
    pub(super) fn bytes(&self) -> usize {
        match self {
            Self::Markdown(t) | Self::Status(t) => t.len(),
            Self::Code {
                text,
                language,
                file,
                ..
            } => text.len() + language.len() + file.len(),
            Self::Table { headers, rows } => headers
                .iter()
                .chain(rows.iter().flatten())
                .map(String::len)
                .sum(),
            Self::Diff {
                file,
                patch,
                summary,
            } => file.len() + patch.len() + summary.len(),
            Self::Tool {
                verb,
                target,
                summary,
                output,
                open,
            } => {
                verb.len()
                    + target.len()
                    + summary.len()
                    + output.len()
                    + open.as_ref().map_or(0, String::len)
            }
            Self::Reasoning { summary, text } => summary.len() + text.len(),
            Self::Attachment {
                name,
                size,
                target,
                unavailable,
            } => {
                name.len() + size.len() + unavailable.len() + target.as_ref().map_or(0, String::len)
            }
            Self::Unknown { kind, description } => kind.len() + description.len(),
        }
    }
    pub(super) fn bound(&mut self, budget: usize) -> (usize, usize) {
        let before = self.bytes();
        let mut left = budget;
        let mut lines = 0;
        let mut cut = |text: &mut String| {
            let mut end = text.len().min(left);
            while !text.is_char_boundary(end) {
                end = end.saturating_sub(1);
            }
            lines += text[end..].bytes().filter(|&b| b == b'\n').count();
            text.truncate(end);
            left = left.saturating_sub(end);
        };
        match self {
            Self::Markdown(t) | Self::Status(t) => cut(t),
            Self::Code {
                text,
                language,
                file,
                ..
            } => {
                cut(language);
                let original = file.len();
                cut(file);
                if file.len() < original {
                    file.clear();
                }
                cut(text);
            }
            Self::Table { headers, rows } => {
                for cell in headers.iter_mut().chain(rows.iter_mut().flatten()) {
                    cut(cell);
                }
                rows.truncate(1024);
                headers.truncate(64);
                for row in rows {
                    row.truncate(64);
                }
            }
            Self::Diff {
                file,
                patch,
                summary,
            } => {
                cut(file);
                cut(summary);
                cut(patch);
            }
            Self::Tool {
                verb,
                target,
                summary,
                output,
                open,
            } => {
                cut(verb);
                cut(target);
                cut(summary);
                if let Some(path) = open {
                    let original = path.len();
                    cut(path);
                    if path.len() < original {
                        *open = None;
                    }
                }
                cut(output);
            }
            Self::Reasoning { summary, text } => {
                cut(summary);
                cut(text);
            }
            Self::Attachment {
                name,
                size,
                target,
                unavailable,
            } => {
                cut(name);
                cut(size);
                cut(unavailable);
                if let Some(path) = target {
                    let original = path.len();
                    cut(path);
                    if path.len() < original {
                        *target = None;
                    }
                }
            }
            Self::Unknown { kind, description } => {
                cut(kind);
                cut(description);
            }
        }
        (before.saturating_sub(self.bytes()), lines)
    }
}
