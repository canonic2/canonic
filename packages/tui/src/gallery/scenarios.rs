//! Maps design boards to executable fixtures and explicit implementation gaps.

use super::Component;
use crate::event::KeyCode;
use crate::picker::Picker;

pub(super) struct Scenario {
    pub id: &'static str,
    pub fixture: Option<usize>,
    pub keys: &'static [KeyCode],
    pub detail: &'static str,
}

macro_rules! live {
    ($id:literal, $fixture:expr) => {
        Scenario {
            id: $id,
            fixture: Some($fixture),
            keys: &[],
            detail: "Interactive fixture",
        }
    };
    ($id:literal, $fixture:expr, $keys:expr, $detail:literal) => {
        Scenario {
            id: $id,
            fixture: Some($fixture),
            keys: $keys,
            detail: $detail,
        }
    };
}
macro_rules! planned {
    ($id:literal, $detail:literal) => {
        Scenario {
            id: $id,
            fixture: None,
            keys: &[],
            detail: $detail,
        }
    };
}

pub(super) const COMPONENTS: [Component; 7] = [
    Component::Picker,
    Component::Chat,
    Component::Editor,
    Component::Dialog,
    Component::Commands,
    Component::Composer,
    Component::CompletionMenu,
];

pub(super) fn index(component: Component) -> usize {
    match component {
        Component::Picker => 0,
        Component::Chat => 1,
        Component::Editor => 2,
        Component::Dialog => 3,
        Component::Commands => 4,
        Component::Composer => 5,
        Component::CompletionMenu => 6,
    }
}

pub(super) fn picker(component: Component) -> Picker {
    let mut names: Vec<_> = entries(component)
        .iter()
        .map(|entry| {
            format!(
                "{} · {}",
                entry.id,
                if entry.fixture.is_some() {
                    "interactive"
                } else {
                    "design pending"
                }
            )
        })
        .collect();
    names.extend(
        ["narrow-40", "ansi16", "no-color", "ascii"].map(|id| format!("{id} · main view profile")),
    );
    Picker::new(format!("{} · scenarios", name(component)), names, Some(0))
}

pub(super) fn entries(component: Component) -> &'static [Scenario] {
    match component {
        Component::Picker => &PICKER,
        Component::Chat => &CHAT,
        Component::Editor => &EDITOR,
        Component::Dialog => &DIALOG,
        Component::Commands => &COMMANDS,
        Component::Composer => &COMPOSER,
        Component::CompletionMenu => &COMPLETION,
    }
}

pub(super) fn name(component: Component) -> &'static str {
    match component {
        Component::Picker => "picker",
        Component::Chat => "chat",
        Component::Editor => "editor",
        Component::Dialog => "dialog",
        Component::Commands => "commands",
        Component::Composer => "composer",
        Component::CompletionMenu => "completion-menu",
    }
}

pub(super) fn board(component: Component) -> &'static str {
    match component {
        Component::Picker => "design/option-picker/README.md · States / Hosts",
        Component::Chat => "design/design-system/README.md · Screens / Dock",
        Component::Editor => "design/editor/README.md · States / Hosts",
        Component::Dialog => "design/dialog/README.md · States",
        Component::Commands => "design/commands-and-bindings/README.md · States / / list",
        Component::Composer => "design/composer/README.md · States / The ↑ key",
        Component::CompletionMenu => "design/completion-menu/README.md · States / Keys & behavior",
    }
}

const COMPLETION: [Scenario; 15] = [
    live!("main", 0),
    live!("files", 1),
    live!("loading", 2),
    live!("still-loading", 3),
    live!("no-match-command", 4),
    live!("no-match-file", 5),
    live!("error", 6),
    live!("partly-failed", 7),
    live!("disabled", 8),
    live!("command-inserted", 9),
    live!("file-inserted", 10),
    live!("narrow", 11),
    live!("long-description", 12),
    live!("mixed-sources", 13),
    live!("doesnt-open", 14),
];
const PICKER: [Scenario; 18] = [
    live!("main", 0),
    live!("details", 1),
    live!("disabled", 2),
    live!("danger", 3),
    live!("filter", 4),
    live!("no-match", 5),
    live!("scrolled", 6),
    live!("multi", 7),
    live!("groups", 8),
    live!("loading", 9),
    live!("narrow", 10),
    live!("done", 11),
    live!("empty", 12),
    live!("error", 13),
    live!(
        "inline",
        0,
        &[],
        "Docked inline placement inside the gallery's terminal; native inline terminal ownership is pending."
    ),
    live!(
        "dialog",
        0,
        &[KeyCode::F(6)],
        "Centered picker host; host draft remains independent."
    ),
    live!(
        "full-screen",
        0,
        &[KeyCode::F(6), KeyCode::F(6)],
        "Full-screen picker host."
    ),
    planned!(
        "native-inline",
        "Own a bounded region in the primary screen and preserve surrounding shell output. Terminal slice pending."
    ),
];
const EDITOR: [Scenario; 16] = [
    live!("main", 0),
    live!("blurred", 1),
    live!("empty", 2),
    live!("wrap", 3),
    live!("selection", 4),
    live!("unicode", 5),
    live!("scrolled", 6),
    live!("pasted", 7),
    live!("undo", 8),
    live!("near-limit", 9),
    live!("limit", 10),
    live!("paste-cut", 11),
    live!(
        "modal",
        0,
        &[KeyCode::F(3)],
        "A modal editor suspends two independent host drafts; Escape restores focus."
    ),
    planned!(
        "composer-host",
        "The design's message editor shares history, stash and queue rules with Composer. Use the complete composer main view for history, stash and queue workflows."
    ),
    planned!(
        "prompt-host",
        "The design's small inline reply owns Enter as confirmation. A dedicated editor prompt host fixture is pending."
    ),
    planned!(
        "external-host",
        "Suspend the terminal for an external editor, then restore it. Process handoff is pending."
    ),
];
const DIALOG: [Scenario; 15] = [
    live!("main", 0),
    live!("nested", 1),
    live!("scroll", 2),
    live!("locked", 3),
    live!("outside", 4),
    live!("removed-return", 5),
    live!("error", 6),
    live!("confirm", 7),
    live!("notice", 8),
    live!("multi-line", 9),
    live!("rename", 10),
    live!(
        "replace",
        0,
        &[KeyCode::F(6)],
        "Replacement closes the old layer and invalidates its saved result ticket."
    ),
    live!(
        "remove",
        0,
        &[KeyCode::F(8)],
        "Host removes the dialog stack and returns focus to the surviving draft."
    ),
    live!(
        "late",
        0,
        &[KeyCode::F(6), KeyCode::F(7)],
        "Stale-result rejection is interactive; the design's timed waiting presentation remains pending."
    ),
    planned!(
        "waiting",
        "Keep a dialog busy while the host checks a source; show the live waiting activity and support cancellation. Timed dialog presentation is pending."
    ),
];
const COMMANDS: [Scenario; 13] = [
    live!("main", 0),
    live!("filter", 1),
    live!("filter-key", 2),
    live!("unavailable", 3),
    live!("no-match", 4),
    live!("keys", 5),
    live!("keys-custom", 6),
    live!("conflict", 7),
    live!("pending", 8),
    live!("pending-hints", 9),
    live!("pending-miss", 10),
    live!("empty", 11),
    planned!(
        "slash",
        "Typing / at the start of the message box opens the command list. Dismissal retains the draft, and execution emits a command ID. Slash routing is pending."
    ),
];
const COMPOSER: [Scenario; 59] = [
    live!("main", 0),
    live!("sending", 1),
    live!("refused", 2),
    live!("refused-kept", 3),
    live!("accepted", 4),
    live!("disconnected", 5),
    live!("queued", 11),
    live!("edit-queued", 12),
    live!("history", 8),
    live!("history-search", 13),
    live!("stashed", 14),
    live!("stash-list", 20),
    live!("editor-open", 15),
    live!("editor-conflict", 16),
    live!("restored", 10),
    live!("up-key", 17),
    live!("box-draft", 6),
    live!("box-queued", 7),
    live!("box-history", 8),
    live!("box-locked", 9),
    live!("box-restored", 10),
    live!("queue-overflow", 18),
    live!("queue-paused", 19),
    live!("stash-swap", 21),
    live!("paste-chip", 22),
    live!("image-chip", 23),
    live!("shell", 24),
    live!("file-reference", 25),
    live!("slash", 26),
    live!("hidden-characters", 27),
    live!("escape-clear", 28),
    live!("queue-sending", 29),
    live!("queue-remove", 30),
    live!("history-empty", 31),
    live!("history-multiline", 32),
    live!("history-edited", 33),
    live!("history-resent", 34),
    live!("history-earlier-session", 35),
    live!("history-oldest", 36),
    live!("stash-empty", 37),
    live!("stash-restored", 38),
    live!("stash-drop", 39),
    live!("editor-returned", 40),
    live!("editor-empty", 41),
    live!("editor-failed", 42),
    live!("queue-stopped", 43),
    live!("escape-cleared", 44),
    live!("escape-rewind", 45),
    live!("quit-armed", 46),
    live!("paste-short", 47),
    live!("paste-selected", 48),
    live!("paste-path", 49),
    live!("shell-output", 50),
    live!("shell-empty", 51),
    live!("file-inserted", 52),
    live!("email", 53),
    live!("returned", 54),
    live!("backslash-newline", 55),
    live!("keys", 56),
];
const CHAT: [Scenario; 13] = [
    live!("main", 0),
    live!("empty", 1),
    live!("streaming", 2),
    live!("error", 3),
    live!("disconnected", 4),
    live!("long", 5),
    planned!(
        "actions-off",
        "Design-system screen 02: activity log with actions disabled; structured activity rendering is pending."
    ),
    planned!(
        "handoff",
        "Screen 03: handoff lens with references and summary; lens routing is pending."
    ),
    planned!(
        "sessions",
        "Screen 04: session browser and preserved per-session state; browser implementation is pending."
    ),
    planned!(
        "pane",
        "Screen 05: conversation in a pane with narrow fallbacks; dedicated composed pane fixture is pending."
    ),
    planned!(
        "anchored-dock",
        "Screen 06: retain the transcript anchor while the dock remains fixed and overlays stream. Full anchor and overlay composition is pending."
    ),
    planned!(
        "answer",
        "Screen 07: structured final answer, code, tool output and completion status. Rich content and activity components are pending."
    ),
    planned!(
        "welcome",
        "Screen 08: session welcome with contextual hints. Dedicated welcome composition is pending."
    ),
];
