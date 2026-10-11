//! Owns the transport-independent tool snapshots and safe source representation.

use std::{collections::BTreeSet, time::Duration};

/// Independent tool lifecycle, never a connection or permission decision.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum State {
    /// Suggested by the host, not scheduled or authorized.
    Proposed,
    /// Waiting to start; reserves a result row without asserting an outcome.
    Queued,
    /// Executing; time and output come from the host.
    Running,
    /// Blocked on the supplied human/system reason, without granting authority.
    Waiting(String),
    /// Host reports an outstanding cancel request; no final outcome yet.
    Cancelling,
    /// Host-confirmed success.
    Completed,
    /// Host-confirmed failure; automatic preview shows three error lines.
    Failed,
    /// Stopped by the supplied actor/reason; distinct from failure.
    Cancelled(String),
    /// Host ended the run before this call supplied a result.
    Interrupted(String),
}
impl State {
    /// Whether this host outcome is final for this call ID.
    #[must_use]
    pub fn terminal(&self) -> bool {
        matches!(
            self,
            Self::Completed | Self::Failed | Self::Cancelled(_) | Self::Interrupted(_)
        )
    }
    pub(super) fn allows(&self, next: &Self) -> bool {
        if self.terminal() {
            self == next
        } else {
            match next {
                Self::Proposed => matches!(self, Self::Proposed),
                Self::Queued => matches!(self, Self::Proposed | Self::Queued),
                _ => true,
            }
        }
    }
}
/// Host output; display caps never become the authoritative external source.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Output {
    /// Plain source, never interpreted as terminal commands or links.
    Text(String),
    /// Code source and display-only initial line number.
    Code { text: String, first_line: u32 },
    /// Host-supplied unified patch; copies preserve the patch exactly.
    Diff(String),
    /// Explicit binary/missing/inaccessible reason, without a fabricated body.
    Unavailable(String),
}
impl Output {
    /// Complete retained semantic output, without display prefixes or numbers.
    #[must_use]
    pub fn text(&self) -> &str {
        match self {
            Self::Text(t) | Self::Code { text: t, .. } | Self::Diff(t) => t,
            Self::Unavailable(_) => "",
        }
    }
    pub(crate) fn text_mut(&mut self) -> &mut String {
        match self {
            Self::Text(t) | Self::Code { text: t, .. } | Self::Diff(t) | Self::Unavailable(t) => t,
        }
    }
}
/// Supplied effects; availability never grants execution authority.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Capabilities {
    /// Host can open/fetch complete details, including during execution.
    pub details: bool,
    /// Host can retry failed/cancelled calls as a new ID.
    pub retry: bool,
    /// Host can consume a semantic clipboard payload.
    pub copy: bool,
}
/// Host-measured progress; a missing total never produces a percentage or bar.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Progress {
    /// Host-reported processed count.
    pub completed: u64,
    /// Known nonzero total, or unknown.
    pub total: Option<u64>,
    /// Safe display unit such as files or bytes.
    pub unit: String,
}
/// Host-owned artifact link. Rendering or selection never opens it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Artifact {
    /// Stable host identity.
    pub id: String,
    /// Display label.
    pub label: String,
    /// Opaque host destination, never interpreted or executed by the UI.
    pub target: String,
    /// Host explicitly enables opening this artifact.
    pub open_available: bool,
}
/// Quiet preview of a child task, without a nested executor or conversation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChildTask {
    /// Host task identity used by the open intent.
    pub id: String,
    /// At most ten already-redacted one-line latest calls.
    pub latest_calls: Vec<String>,
    /// Authoritative total call count; may exceed the retained preview.
    pub total_calls: usize,
}
/// Safe display snapshot for one host operation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Call {
    /// Stable nonempty host identity, at most 128 bytes and without controls.
    pub id: String,
    /// Monotonically increasing snapshot revision; zero is a valid initial value.
    pub revision: u64,
    /// Read/search/edit/write/run/fetch/ask, or a generic tool fallback.
    pub verb: String,
    /// Safe path, command or query summary; no execution schema is interpreted.
    pub target: String,
    /// Unknown tool provider/source label.
    pub source: String,
    /// Unknown tool operation name.
    pub name: String,
    /// Redacted one-line unknown-tool argument summary.
    pub arguments: String,
    /// Host interpretation of the result; the UI never parses output to invent it.
    pub summary: String,
    /// Tool lifecycle, independent of connection and turn state.
    pub state: State,
    /// Host elapsed duration; live labels start at one second.
    pub elapsed: Duration,
    /// Retained source bounded to 1 MiB at standalone construction/update.
    pub output: Output,
    /// Explicit host effects; selecting a card never authorizes an operation.
    pub capabilities: Capabilities,
    /// Host-supplied successor ID. The prior outcome remains historical.
    pub retried: Option<String>,
    /// Bytes omitted at the display boundary; hosts keep the full source.
    pub omitted_bytes: usize,
    /// Original supplied output line count, retained even after source byte capping.
    /// Zero derives the count from output; a host may supply a larger authoritative count.
    pub total_lines: usize,
    /// Optional host-measured progress.
    pub progress: Option<Progress>,
    /// At most sixteen host-linked artifacts.
    pub artifacts: Vec<Artifact>,
    /// Optional host-owned child-task summary.
    pub child_task: Option<ChildTask>,
    /// Host cannot currently establish a live call's result; last elapsed time stays frozen.
    pub result_unknown: bool,
}
impl Call {
    /// Minimal queued call. Host fills summary, lifecycle and explicit capabilities.
    ///
    /// ```
    /// use canonic_tui::tool_call_card::{Call, State, ToolCallCard};
    /// let mut call = Call::new("test-1", "run", "npm test -- digest");
    /// call.state = State::Completed;
    /// call.summary = "6 passed".into();
    /// let card = ToolCallCard::new(call).unwrap();
    /// assert_eq!(card.call().summary, "6 passed");
    /// ```
    #[must_use]
    pub fn new(id: &str, verb: &str, target: &str) -> Self {
        Self {
            id: id.into(),
            revision: 0,
            verb: verb.into(),
            target: target.into(),
            source: String::new(),
            name: String::new(),
            arguments: String::new(),
            summary: String::new(),
            state: State::Queued,
            elapsed: Duration::ZERO,
            output: Output::Text(String::new()),
            capabilities: Capabilities::default(),
            retried: None,
            omitted_bytes: 0,
            total_lines: 0,
            progress: None,
            artifacts: vec![],
            child_task: None,
            result_unknown: false,
        }
    }
    pub(crate) fn validate(&self) -> Result<(), &'static str> {
        self.validate_extensions()?;
        if !valid_id(&self.id)
            || self
                .retried
                .as_deref()
                .is_some_and(|id| !valid_id(id) || id == self.id)
        {
            return Err("invalid call identity");
        }
        if [
            &self.verb,
            &self.target,
            &self.source,
            &self.name,
            &self.arguments,
            &self.summary,
        ]
        .iter()
        .any(|s| s.len() > 4096)
        {
            return Err("tool metadata exceeds 4096 bytes");
        }
        Ok(())
    }
    fn validate_extensions(&self) -> Result<(), &'static str> {
        let mut ids = BTreeSet::new();
        if self
            .progress
            .as_ref()
            .is_some_and(|p| p.total == Some(0) || p.unit.len() > 128)
            || self.artifacts.len() > 16
            || self.artifacts.iter().any(|a| {
                !valid_id(&a.id)
                    || !ids.insert(&a.id)
                    || a.label.len() > 128
                    || a.target.len() > 4096
                    || a.target.chars().any(char::is_control)
            })
            || self.child_task.as_ref().is_some_and(|c| {
                !valid_id(&c.id)
                    || c.latest_calls.len() > 10
                    || c.latest_calls.iter().any(|s| s.len() > 4096)
            })
        {
            return Err("invalid tool extension metadata");
        }
        Ok(())
    }
    pub(crate) fn sanitize(&mut self) {
        // Cap raw source before escaping so hostile controls cannot amplify an unbounded input.
        self.bound_output(1024 * 1024);
        for s in [
            &mut self.verb,
            &mut self.target,
            &mut self.source,
            &mut self.name,
            &mut self.arguments,
            &mut self.summary,
        ] {
            *s = label(s);
        }
        match &mut self.state {
            State::Waiting(s) | State::Cancelled(s) | State::Interrupted(s) => *s = label(s),
            _ => {}
        }
        *self.output.text_mut() = safe(self.output.text_mut());
        self.bound_output(1024 * 1024);
        if let Some(progress) = &mut self.progress {
            progress.unit = label(&progress.unit);
        }
        for artifact in &mut self.artifacts {
            artifact.label = label(&artifact.label);
        }
        if let Some(child) = &mut self.child_task {
            for call in &mut child.latest_calls {
                *call = label(call);
            }
        }
    }
    pub(crate) fn bound_output(&mut self, budget: usize) -> usize {
        self.total_lines = self.total_lines.max(self.output.text().lines().count());
        let text = self.output.text_mut();
        let mut end = text.len().min(budget);
        while !text.is_char_boundary(end) {
            end = end.saturating_sub(1);
        }
        let omitted = text.len() - end;
        text.truncate(end);
        self.omitted_bytes = self.omitted_bytes.saturating_add(omitted);
        omitted
    }
    pub(crate) fn bound(&mut self, budget: usize) {
        let mut left =
            budget.saturating_sub(self.id.len() + self.retried.as_ref().map_or(0, String::len));
        for text in [
            &mut self.verb,
            &mut self.target,
            &mut self.source,
            &mut self.name,
            &mut self.arguments,
            &mut self.summary,
        ] {
            let mut end = left.min(text.len());
            while !text.is_char_boundary(end) {
                end = end.saturating_sub(1);
            }
            self.omitted_bytes = self.omitted_bytes.saturating_add(text.len() - end);
            text.truncate(end);
            left = left.saturating_sub(end);
        }
        if let State::Waiting(text) | State::Cancelled(text) | State::Interrupted(text) =
            &mut self.state
        {
            let mut end = left.min(text.len());
            while !text.is_char_boundary(end) {
                end = end.saturating_sub(1);
            }
            self.omitted_bytes = self.omitted_bytes.saturating_add(text.len() - end);
            text.truncate(end);
            left = left.saturating_sub(end);
        }
        // Extension identities and destinations are atomic; omit whole entries when bounded.
        if let Some(progress) = &self.progress {
            if progress.unit.len() > left {
                self.progress = None;
            } else {
                left -= progress.unit.len();
            }
        }
        self.artifacts.retain(|a| {
            let bytes = a.id.len() + a.label.len() + a.target.len();
            if bytes > left {
                self.omitted_bytes = self.omitted_bytes.saturating_add(bytes);
                false
            } else {
                left -= bytes;
                true
            }
        });
        if let Some(child) = &mut self.child_task {
            let bytes = child.id.len() + child.latest_calls.iter().map(String::len).sum::<usize>();
            if bytes > left {
                self.omitted_bytes = self.omitted_bytes.saturating_add(bytes);
                self.child_task = None;
            } else {
                left -= bytes;
            }
        }
        self.bound_output(left);
    }
    /// Semantic command/target and output for copying, independent of preview caps.
    #[must_use]
    pub fn copy_text(&self) -> String {
        let head = if self.known() {
            format!("{} {}", self.verb, self.target)
        } else {
            format!("tool {} · {} {}", self.source, self.name, self.arguments)
        };
        if self.output.text().is_empty() {
            head
        } else {
            format!("{head}\n{}", self.output.text())
        }
    }
    pub(super) fn known(&self) -> bool {
        matches!(
            self.verb.as_str(),
            "read" | "search" | "edit" | "write" | "run" | "fetch" | "ask" | "index" | "task"
        )
    }
    pub(crate) fn bytes(&self) -> usize {
        [
            &self.id,
            &self.verb,
            &self.target,
            &self.source,
            &self.name,
            &self.arguments,
            &self.summary,
        ]
        .iter()
        .map(|s| s.len())
        .sum::<usize>()
            + self.output.text_mut_ref().len()
            + match &self.state {
                State::Waiting(s) | State::Cancelled(s) | State::Interrupted(s) => s.len(),
                _ => 0,
            }
            + self.retried.as_ref().map_or(0, String::len)
            + self.progress.as_ref().map_or(0, |p| p.unit.len())
            + self
                .artifacts
                .iter()
                .map(|a| a.id.len() + a.label.len() + a.target.len())
                .sum::<usize>()
            + self.child_task.as_ref().map_or(0, |c| {
                c.id.len() + c.latest_calls.iter().map(String::len).sum::<usize>()
            })
    }
}
impl Output {
    fn text_mut_ref(&self) -> &str {
        match self {
            Self::Unavailable(s) => s,
            _ => self.text(),
        }
    }
}
/// Host-declared overlapping calls, ordered by start, with one group identity.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ToolGroup {
    /// Stable host overlap-group identity.
    pub id: String,
    /// Monotonically increasing group snapshot revision.
    pub revision: u64,
    /// Up to 128 unique calls, in host start order; retries follow their parent.
    pub calls: Vec<Call>,
}
impl ToolGroup {
    pub(crate) fn validate(&self) -> Result<(), &'static str> {
        let mut ids = BTreeSet::new();
        if !valid_id(&self.id) || self.calls.is_empty() || self.calls.len() > 128 {
            return Err("invalid group identity or call count");
        }
        for call in &self.calls {
            call.validate()?;
            if !ids.insert(&call.id) {
                return Err("duplicate call identity");
            }
        }
        Ok(())
    }
}
fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 128 && !id.chars().any(char::is_control)
}
pub(crate) fn safe(text: &str) -> String {
    text.chars()
        .flat_map(|c| match c {
            '\u{1b}' => vec!['␛'],
            '\n' | '\t' => vec![c],
            c if c.is_control() => format!("[U+{:04X}]", u32::from(c)).chars().collect(),
            c => vec![c],
        })
        .collect()
}
pub(crate) fn label(text: &str) -> String {
    crate::text::clean(&safe(text).replace(['\n', '\t'], " "), 4096, false)
}
