# Tool Call Card

## Purpose

Tool Call Card presents one identified operation inside a [Chat](chat.md) turn.
`tool_call_card` owns lifecycle presentation, safe textual output, local expansion,
bounded previews and capability-gated actions. `ToolGroup` composes independent
cards in host order. Neither acquires a terminal, clock, clipboard, storage,
permission authority nor execution adapter. Hosts redact sensitive material before
supplying it, describe outcomes and acknowledge actual effects.

## Current status

Reusable cards/groups, Chat integration and 79 gallery scenarios are implemented,
including 20 whole-run activity/cancellation scenarios. Human visual acceptance
and real host adapters remain separate from engineering checks.

## Behavior and state transitions

### Extended lifecycle and host metadata

Proposed is an unscheduled suggestion, with dim head, `proposed` metadata and
`○ not started · proposed by host`. It is distinct from queued, which reserves
an empty result row. Cancelling is an outstanding host request, never a final
outcome. Interrupted is final without a result, uses warn `! no result · <reason>`
and permits retry only when enabled. Failed alone uses `✕`; cancelled uses dim `–`.
Whole-run cancellation and connection state follow [Run Activity](run-activity.md).

Optional progress is a host processed count, unit and optional nonzero total.
With a total, show a bounded bar and the exact count; clamp only the bar when the
processed count exceeds the total. Without a total, show the count “so far”,
elapsed time and working dots. Never estimate totals or derive success from a bar.

The host redacts arguments before providing them; `•••` is display text rather
than a UI secrecy detector. Escape bytes are retained as visible dim `␛`, without
executing terminal commands. Generated ASCII uses `^[` for that marker.

At most sixteen artifacts supply stable IDs, labels, opaque host destinations and
explicit open availability. They render on one line beneath the result. Opening
returns an identified, acknowledged host intent. A child task supplies a stable
task ID, up to ten quiet latest-call lines and an authoritative total-call count.
Its automatic body shows the last two; expansion shows the retained ten. Enabled
details for a child task means `o open task`, returning the task ID to the host.
Neither preview causes task execution or automatic navigation.

Host group order is authoritative. Reordering retains member expansion and
selected identity. UI updates do not sort by state, completion time or label.
Missing historical members remain an invalid replacement; an explicit new group
is needed to discard history. Existing member revisions/transitions still validate
atomically before a reordered envelope is accepted.

### Snapshots and source limits

A `Call` supplies a stable nonempty ID (at most 128 bytes, without controls), monotonically increasing revision, verb,
target, unknown-tool source/name/argument summary, host summary, elapsed duration,
output, original line count and capabilities. Original line counts survive byte
capping; hosts may supply an authoritative larger count. Output is text, numbered code, unified diff or an explicit
unavailable reason (binary, missing, empty or inaccessible). The host retains full
output. The display model accepts at most 1 MiB of output per call; excess bytes
are counted and disclosed. Details capability addresses the host's complete source.
An empty textual output renders no body, and the result still names the state.
No summary, success or permission decision is inferred from output.
Verb, target, source, name, arguments and summary accept at most 4,096 bytes each;
displayed reasons are cleaned and bounded to 4,096 bytes.

States are Proposed, Queued, Running, Waiting(reason), Cancelling, Completed,
Failed, Cancelled(reason) and Interrupted(reason). Proposed may queue or start;
queued may run, wait or end directly. Active states may move between running,
waiting and cancelling or end. Terminal outcomes cannot return to running under the
same ID. Terminal state and cancellation reason stay fixed on later revisions;
output and metadata may still update. A retry is a new ID, with the prior failed/cancelled card retained,
dimmed and folded and a host-supplied `retried` reference. Updates reject foreign
IDs, stale revisions, invalid transitions and changed retry history. Snapshot
replacement in Chat additionally uses its conversation/message/part revision.
Elapsed time comes from the host, never a system clock read.

## Rendering contract

The caller supplies placement; Chat adds a two-cell turn indent. At widths of
50 or more, verbs occupy eight cells plus a separator; read/search/fetch are dim,
paths use info and shorten in the middle retaining the filename, commands are
bold and search queries are quoted. Unknown verbs use `tool`, source, name and
arguments. Under 50 columns the verb occupies four cells (`search` becomes
`find`), paths use their filename and duration moves to the result row. Metadata
is right aligned when wide. Completed durations may include subsecond values;
live timers appear from one second.

Every card reserves head and result rows, including a blank queued result row.
Running uses `··· running`, elapsed and supplied output line count. Waiting uses
`! waiting for …`; completed uses `→ ✓` and the host summary; failed uses `→ ✕`
and bold error styling; cancellation uses plain dim `– cancelled` and its reason,
only after host confirmation. Pending cancellation belongs to run activity.
Waiting displays supplied text only; any question/picker/dialog is host composition.

Local expansion has three modes: automatic, collapsed and expanded. Automatic
failed cards show the first three error lines after the result, waiting cards open
up to twenty lines, and ordinary text cards show the last three above the result.
Collapsed code/diff cards show counts/summary only. Explicit expansion shows the
first ten and last ten source lines, with a counted marker between; up to twenty
lines show wholly. Explicit collapse hides failed/waiting previews. Counts identify
every hidden source line and omitted bytes. Details/copy use the entire retained
source, never the preview, prefixes, indentation or display line numbers.

Bodies use a quiet vertical rule under the target column, muted text, code surface
and dim numbers, and unified diff signs with add/delete roles. Diff headers remain
text; valid hunk ranges determine display numbers. Malformed patches stay readable
as raw patch text. Below 50 columns numbers drop and long lines wrap with a
continuation marker, retaining diff signs on the initial row. Failed lines wrap
at every width. Live collapsed text keeps three physical rows, clipping long lines
with a disclosed display marker. Other previews have a 256-row wrapping budget
with an explicit marker. Controls are visibly escaped; output URLs/OSC/ANSI are
text and never create terminal links. ASCII chrome uses `->`, `ok`, `x`, `...`,
`|`, `-`, `+` and `\\`; original Unicode source remains source. ANSI 16 uses only
palette/default colors. NO_COLOR uses default colors and attributes, bold failure,
plain cancellation and reversed selected head instead of a colored selection bar.

Streaming snapshots count immediately, but displayed output coalesces to at most
ten updates per second using caller-supplied ticks. First output and final outcomes
flush immediately. Cards preserve identity and expansion across updates. The
three-row tail and omission row remain reserved while streaming once output exists.

## Concurrency

A group supplies a stable group ID and calls in authoritative host order; updates
may reorder by ID and never sort implicitly. Its dim header says `N at once`, with outcome counts and maximum
elapsed when terminal. Automatic all-completed groups fold to one summary. Mixed
terminal groups show failures/cancellations and count successful hidden siblings.
Live groups above six calls show the first five plus all failures and a counted
`+N more` row. Enter on the group expands all members or restores the automatic
fold. Each visible member remains independently selectable and expandable.

## Input, focus and host actions

Cards receive commands only in Chat Select mode (`ctrl+x v`); ordinary text,
including `o`, `r`, `y` and `c`, goes to Composer. Up/Down navigate visible blocks;
Enter toggles card/group expansion without losing the selected head's viewport
position. Escape leaves selection/details; Escape in Compose retains Chat's
whole-turn cancel action. There is no per-card interrupt or grant action.

`o` emits Details only when enabled, including during running. `r` emits Retry
only for enabled failed/cancelled/interrupted cards without a retry successor. `y` emits Copy
of call plus complete retained output; `c` emits Copy of output alone when text
exists. Requests include opaque instance ticket, call ID and revision; repeats
and duplicate requests are suppressed until acknowledgement. Unknown/stale/foreign
acknowledgements do nothing. Revision changes invalidate pending effects. Failure
is visible and allows another deliberate request. Hints list available actions
only. The host inserts a retry below the original, updates its `retried` reference
and acknowledges after accepting the retry; refusal changes no history.

`a` opens the selected card's local artifact chooser when an artifact can be
opened. Arrows choose a supplied stable ID; Enter emits its host open intent.
Escape closes the chooser without opening. It reuses Option Picker and Dialog;
the card's Composer draft, caret, selection and anchor remain intact. Hosts may
also dispatch a deliberate artifact ID through `Chat::open_tool_artifact`.

The gallery's default details host is a full-screen retained-output viewer with
scrolling, literal search (`/`), find-next and acknowledged `y` copy-all. Escape
returns to the identical Chat selection/anchor and Composer draft/caret. Real
hosts may provide another details view or fetch omitted output; the reusable
component emits intent and does not fetch it.

## Implementation design

Chat consumes `Content::ToolCard` and `Content::ToolGroup`; the generic `Tool`
projection remains compatible. Card/group view state is scoped to message/part
identity, removed on eviction/reset and never shared between conversations.
Chat's source budget remains 64 KiB per turn; card output beyond it is explicitly
counted. Its independent row/cache limits also apply. Hosts needing all output
provide Details. Tool intents retain message/part identity and the card ticket;
acknowledgement restores no editor snapshot over a newer draft. Group snapshots
require a nonempty group and reject duplicate IDs or more than 128 calls.
No render produces host effects.

## Host boundary

Hosts own execution, permissions, redaction, storage, clipboard and complete output.
Cards own no event loop or terminal lease. They render on the caller's frame and
emit identified intent only from deliberate selected-scope commands. Real consumer
hosts revalidate availability/authority before executing and acknowledge success,
refusal or failure; a gallery acknowledgement is only local fixture state.

## Acceptance criteria

- **TC-01:** all nine states and text/code/diff/unavailable/unknown bodies convey
  host outcomes, safe text and precise omissions in wide/narrow/color profiles.
- **TC-02:** concurrent updates preserve host order, member identity, expansion,
  success folding and visible failures; large streams coalesce without losing retained source.
- **TC-03:** selection, expansion, capability gating, full copy, details/retry
  intents and stale/foreign/duplicate acknowledgements preserve draft/caret/anchor.
- **TC-04:** gallery maps every state and interaction to deterministic executable
  scenarios, including errors and fallbacks; PTY navigation/resize/restoration work.
- **TC-05:** library-only, public API/consumer compilation and engineering gates
  pass; human visual and broader platform acceptance are recorded independently.

## Remaining gaps

Implementation and executed evidence are recorded in [Verification](../verification.md).
Question Form, permission review, Dialog timing and real consumer execution are
separate capabilities.
