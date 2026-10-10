# Composer

## Purpose

The composer owns one session draft, its queue, recall, stash, paste chips,
completion routing and editor-return conflicts. Hosts own repository/session
identity, transport, shell execution, clipboard reads, durable writes and the
terminal lease. The gallery exercises those boundaries with deterministic host
responses. This contract includes the implementation layout and interaction
details needed to build the public component independently.

## Current status

The complete reusable local workflow and explicit foreground-editor host adapter
are implemented. Gallery responses remain deterministic simulations. Engineering
results and human acceptance are separate in [Verification](../verification.md).

## Behavior and state transitions

### Editor integration

The [Editor contract](editor.md) owns text editing, selection, undo,
limits and rendering. Composer adds session lifecycle, recall and atomic payloads.
Its default expanded payload limit is 64 KiB, configurable with `Composer::new(limit)`;
a draft supports at most 256 inline payloads. Submission starts a fresh editor.

### Input precedence

A visible completion, history/stash picker or conflict dialog consumes input
before the box. Then composer lifecycle bindings run, followed by box-mode
navigation, Enter handling and generic editor editing. Release events never
execute destructive operations; repeated Enter does not submit. Typing or
navigation cancels an armed clear/quit confirmation.

| Key | Behavior |
| --- | --- |
| Enter | Accept active list; clean hidden controls; replace trailing line-end backslash with newline; ignore blank; save queued edit; queue while busy; otherwise send |
| Ctrl-J, Alt-Enter, distinctly encoded Shift-Enter | Insert newline, never submit |
| Up | Edit through visual lines; on first row go to byte zero; next press enters newest queue entry, then older queue entries and history |
| Down in history | Newer entry; past newest restores exact session draft |
| Ctrl-R | Search history; Enter recalls without submitting |
| F1 | Open composer key help |
| Ctrl-V | Request a host clipboard image |
| Ctrl-X s | Stash session draft and clear box |
| Ctrl-X u | Restore newest stash, or swap with a nonempty draft |
| Ctrl-X Enter | Resume paused queue and send one eligible entry; request stopping active work after its current step |
| Ctrl-X e | Request external editor |
| Ctrl-X b | Request backgrounding shell work |
| Ctrl-D in queued edit | Remove queued entry into stash and restore session draft |
| Ctrl-D with text | Delete next grapheme |
| Ctrl-C with text, idle | Clear into recall history immediately |
| Ctrl-C or Ctrl-D, empty and idle | Arm quit; same key again within 1.5 seconds saves and requests quit |

The Ctrl-X leader expires after three seconds; an unknown continuation is
consumed with a notice. Escape closes the nearest list/dialog, restores recall,
cancels editor waiting, or stops active work and pauses the queue. In shell mode,
idle Escape returns to message mode while keeping text. A selected draft first
clears selection. Otherwise, Escape on text arms clearing; a second press within
1.5 seconds clears into recoverable recall history. On an empty idle draft,
Escape twice recalls the last message for replacement. A rewind intent is emitted
only when that recalled draft is submitted; the host decides how files change.
A single Escape never drops content.

Gallery navigation uses F10 from the composer because Escape implements these
product actions. F10 preserves the current fixture and draft; F10 again returns.
Escape still navigates scenario picker → component picker with retained search,
selection and viewport. Other components retain their own local Escape rules.

### Submission and acknowledgement

`update` returns at most one `Submission`: opaque attempt ticket, lifetime and
revision, expanded text, input mode and lossless document. The document includes
chip content and opaque attachment references. Enter empties the box immediately.
New edits belong to a fresh draft. The host projects the fixed turn, executes the
request and acknowledges that exact ticket. Acceptance does not erase newer
text. Duplicate, foreign and stale acknowledgements do nothing.

A refusal restores the exact document only when the submitted fresh box is still
untouched and empty. Otherwise it stashes the refused document and preserves the
newer draft, even if the newer draft became empty through editing. A refused
snapshot pauses queue delivery. A return while the box is locked advances its
version, so the editor's later return requires conflict resolution.

Editing, undo/redo and host replacement advance revision; cursor movement does
not. Reset establishes a new lifetime and invalidates submission, editor and
save callbacks. Hosts cancel their own outstanding transport/process work.

### Queue

Busy includes an accepted running turn and an unacknowledged submission.
Enter queues complete draft records in FIFO order and clears the box. A queue
holds at most 128 entries; reaching capacity refuses the new item and retains
its draft with a notice. Queue rows show at most three entries, then `+N more`.
Positions renumber after sending/removal. An entry being edited says
`editing below`; scheduling identifies it by stable identity, not its number.

A normal successful turn end returns one eligible queued submission. It does
not batch the queue. A held edit is skipped; later eligible entries may send.
Enter saves the edited copy into its original slot and returns to the session
draft. Escape discards the edit and keeps the original. Ctrl-D removes the slot,
stashes the edited copy, and restores the session draft. A blank saved edit is
removed into the stash.

A failed turn, refusal or stop pauses delivery. Later successful turn-end
callbacks do not resume it. Ctrl-X Enter explicitly resumes and sends one.
If a submission still awaits acknowledgement, manual delivery waits for that
response. Restart always restores the queue paused and emits no submission.

### History and stash

Accepted history is scoped by the host to one repository. `set_history_entries`
accepts lossless documents, message/shell modes and optional origin labels;
this-session entries precede earlier sessions, preserving their supplied order.
`set_history` remains a bounded text-only convenience API. Message and shell
recall pools are separate, at most 500 entries each. Resending identical expanded
text moves it to the front instead of duplicating it. Origin labels such as
`yesterday` are host-provided and sanitized. Slash commands never enter sent
history. Locally cleared drafts remain recoverable through recall.

Entering recall sets the entire draft editor aside, including cursor, selection,
undo and viewport. Recalled content is a copy. Escape or Down past newest restores
the original editor exactly. Typing, deleting or undoing in a history copy adopts
it as the session draft and stashes a nonempty set-aside draft with a notice.
The source history remains unchanged. The oldest entry does not wrap.

The stash is newest first in its picker, with first-line previews and ages.
Ctrl-X s ignores an empty draft with a notice. Ctrl-X u restores text, cursor,
chips and input mode; over existing text it swaps, and another press swaps back.
The stash also receives refused snapshots, removed queue entries, set-aside
drafts on recall edits, and the unused version from editor conflicts.
It holds 50 entries; inserting the fifty-first drops the oldest with a notice.
This explicit capacity rule is the exception to indefinite retention.
In the stash picker, d drops the selected entry and u undoes the latest drop.
Undo remains available until the picker closes. Enter restores/swaps.

### Paste and attachments

Bracketed paste is always data. It never submits, opens slash/file lists or
switches shell mode, regardless of prefixes or trailing newline. Normalize
CRLF/CR to LF. A paste of more than three lines or 800 characters becomes
`[paste N · L lines]`; its complete content remains attached to one atomic
editor marker. A chip is one cursor step, backspace and undo transaction.
Ordinary short paste is a single text transaction.

`insert_image` accepts an opaque clipboard attachment ID from the host. Image
chips are numbered separately; optional dimensions appear in the label. The host owns clipboard decoding, dimensions,
file-path fallback, payload lifetime and model support. The submission document,
history, queue, stash and saved state retain the attachment ID. External editing
expands text chips into editable content; unchanged image placeholders retain
their opaque references on return. Edited/removed image placeholders become
ordinary text or disappear.

Zero-width space, directional format controls and byte-order marks are removed
on Enter, including inside pasted content. That Enter does not submit; a notice
allows inspection before another Enter sends. ZWJ/ZWNJ used in ordinary scripts
and joined emoji are retained.

Hosts without bracketed paste can use `PasteBurst`: plain presses within eight
milliseconds are buffered; four or more form one paste, including Enter as a
newline. Twenty milliseconds idle flushes the burst. Short typing remains key
events; modified/control events flush before delivery. The gallery uses this
decoder only for the composer and bypasses it for active pickers.

### Shell and completion

Typing ! in an empty message draft changes mode to shell and consumes the prefix.
The rule is yellow `shell`; the mode is carried in submissions and queued
records. Enter returns a shell request to the host. Shell mode remains until
idle Escape or Backspace on an empty box; leaving with text keeps it as a message.
Hosts run commands in their repository and environment, manage cancellation and
backgrounding, and add output to the conversation. `Chat::set_shell_output`
updates the matching shell turn; output folds after 20 lines and
`toggle_shell_output` expands or collapses that turn. Execution and authorization
never occur inside composer state.

The [Completion Menu contract](completion-menu.md) owns typed slash/file
queries, result tickets, insertion and input precedence. Composer owns the draft
revision and routes command selection into host intents or its built-in stash list.

### Saved state and external-editor host boundary

Changes schedule a Save intent after 500 milliseconds through the injected clock.
Cursor movement also schedules a save. Hosts drain intents and acknowledge the
exact save ticket. Failed writes keep all in-memory content and schedule a retry.
Quit emits a current Save snapshot before Quit; hosts finish that write before
closing. Repository history/stash and session draft/queue are scoped by the
host's storage key, not a process-global singleton.

`SavedComposer` contains the session draft, queue, histories, lossless history
documents/origins and stash. It also protects pending submission, partial queued
edit and unresolved editor-return copies. `write_to/read_from` provide bounded,
versioned binary encoding on caller-owned streams. Decoding limits the configured
editor size to 1 MiB and aggregate string allocation to 256 MiB. The header is
`CANONIC-COMPOSER` plus schema byte 1; lengths are little-endian u64. Unknown
schema, invalid UTF-8, excessive counts and truncation fail visibly. The host
owns private file permissions, atomic replace, repository/session keys and I/O.

`restore_saved` validates the complete snapshot before changing state. It
restores draft/chips/cursor/mode, pauses the queue, invalidates old tickets,
and reports restoration once. Recovery copies go into the stash; nothing sends
or resumes an editor automatically. Durable snapshots omit undo/selection,
while in-process recall preserves them.

Ctrl-X e returns an `EditorRequest` with ticket, current version, expanded text, original attachment document
and label, then locks ordinary input. `editor_command` resolves supplied VISUAL
before EDITOR. The optional `run_editor` host adapter creates an exclusive
owned temp file (0600 on Unix), passes its path as a separate shell argument,
waits for the configured editor and cleans the file on success/failure.
Cleanup failures are returned alongside operation failures rather than hidden.
The command is trusted host configuration; the adapter does not discover it
or run implicitly. `TerminalSession::handoff` restores terminal modes before
a foreground callback and reacquires afterward, returning operation and terminal
results separately. GUI scheduling and process cancellation remain host duties.

`finish_editor` accepts only the active ticket. Missing command, empty file,
nonzero exit, oversized content, cancellation or read failure keeps the draft
and reports a reason. An unchanged version accepts the returned text; a changed
version opens the shared conflict dialog. Cancel waiting invalidates later
returns. Neither version is silently overwritten.

## Implementation design

### Implementation owners

| Source | Responsibility |
| --- | --- |
| [composer.rs](../../src/composer.rs) | Draft lifetime/revision, submission tickets and acknowledgement recovery |
| [lifecycle](../../src/composer/lifecycle.rs), [modes](../../src/composer/modes.rs) | FIFO queue, held edits, stash, cancellation/leader bindings and box rules |
| [history](../../src/composer/history.rs), [lists](../../src/composer/lists.rs) | Repository recall, origin labels, search/stash/key pickers and drop undo |
| [payload](../../src/composer/payload.rs), [editor atoms](../../src/editor/atoms.rs) | Lossless atomic text/image chips and hidden-control cleanup |
| [completion](../../src/composer/completion.rs), [paste burst](../../src/composer/paste_burst.rs) | Typed-prefix queries/results and optional unbracketed-paste decoding |
| [saved](../../src/composer/saved.rs), [codec](../../src/composer/codec.rs) | Debounced snapshots, validation, recovery copies and bounded stream encoding |
| [handoff](../../src/composer/handoff.rs), [editor host](../../src/composer/editor_host.rs) | Editor ticket/conflict state and explicit foreground process adapter |
| [Chat](../../src/chat.rs), [runtime](../../src/runtime.rs) | Turn projection/fixed dock and independently owned terminal lease |
| [gallery composer](../../src/gallery/composer.rs) | Deterministic host simulation and individual scenario fixtures |

### Implementation layout

The composer dock occupies five rows: a message rule, three editor rows and a
footer rule. The host places one activity row immediately above it and one key
status row immediately below. Queue and picker content grow upward into the
conversation; they never change the dock's height or vertical position. If the
terminal is too small, the host clamps the allocated regions.

```text
you · 20:10 · sending
  run the digest tests

queued  1  update the changelog
        2  run the full suite
2 queued · sent one per turn · ↑ edits
message ──────────────────────────────────────     ↑ history
<editable draft row 1>
<editable draft row 2>
<editable draft row 3>
────────────────────────────────────────── acme-api#276 ~chat
actions on · enter queues
```

The footer's repository/session label and transcript timestamps are host inputs.
The footer label uses bold information color; its rule stays cyan.
The normal submitted turn uses bold `you`, a dim timestamp and dim `sending`.
Acceptance removes only the delivery suffix. A refusal uses an error suffix
and a sanitized reason; ordinary sending has no yellow activity row.

| Box content | Top rule | Enter | Escape |
| --- | --- | --- | --- |
| Session draft | Solid cyan `message` | Send, or queue while busy | Follow the cancellation ladder |
| Queued edit | Dashed blue `queued`, position/total | Save the same queue slot | Keep its original and restore the session draft |
| History copy | Dashed blue `history`, position/total and optional origin | Send another copy | Restore the session draft |
| External editor | Dim solid `message`, `in <editor>` | Ignore | Cancel waiting; keep text |

The draft rule's right hint prioritizes armed cancellation, `↑ history` at byte
zero, returned text, then stash count. Zero stash count is hidden. Recall uses
Unicode `┄`; ASCII uses dots instead of the draft's hyphens. The `line-info`
token is RGB #495F81, indexed 60 and faint blue in ANSI 16. The draft line is
#2E7479 in RGB. NO_COLOR retains the solid/dashed distinction. Chips use a quiet
surface and muted label; a chip at the caret uses selection fill and bold accent,
with payload size on the rule. The rule adds ↑/↓ more when editor rows are hidden.

History, stash and completion reuse the option picker above the dock, up to nine
rows high. Filtering, paging and resize belong to that picker. Stash drops retain
the opening height. Editor conflicts use the shared centered dialog chrome:
the question names both versions, 1 selects editor text, 2 selects box text,
Enter confirms the highlighted version, and Esc keeps the box. The other version
always goes to the stash. The dialog uses surface fills in RGB/indexed modes and
boxes in ANSI 16/NO_COLOR; the background fades without moving the dock.

`Chat::render_docked_with` composes the unframed conversation and fixed dock.
`render_with` retains the framed Chat host. A custom host calls
`render_above`, `render_with` and finally `render_overlay` on caller-owned frames.
The final overlay pass preserves shared dialog fading and centering.

## Host boundary

Hosts own repository/session identity, transport, command/file discovery, shell
execution, clipboard decoding, attachment lifetime and durable storage. They
acknowledge exact submission/save tickets and scope stored records explicitly.
Foreground editor execution and terminal handoff are opt-in host adapters;
ordinary Composer update/render does not run processes or acquire a terminal.

## Acceptance criteria

All composer entries in the [scenario registry](../features/gallery-designs.md) are runnable.
Main, sending/refusal, queue/held edit/pause/overflow, history/search, stash/list/
swap, editor/return conflict, restart, paste/image, shell, slash/file, hidden-control
and Escape fixtures exercise the same public component APIs. F5 accepts or ends
a turn, F6 refuses or fails it, F7 resets draft lifetime, F8 sends a stale response,
F9 changes availability, F11 returns simulated editor text and F12 restores saved
state. F3 expands/collapses shell output. These harness controls are outside component
layout acceptance. There are 59 named composer entries, including aliases,
covering 57 distinct initial states plus four main-view presentation profiles.

Shared editing, paste transactions and text/cursor bounds are owned by
[Editor E-01–E-03](editor.md#acceptance-criteria). Composer additionally verifies
pasted prefixes/burst newlines never execute, atomic payloads retain their
content, and recall/chip meaning survives NO_COLOR/ASCII.

- **E-04:** Submission clears immediately; delayed/foreign/stale responses cannot
  erase a newer draft or duplicate execution.
- **E-05:** Queue scheduling, stash swaps, recall, editor cancellation/conflicts
  and restart preserve recoverable documents, cursors, modes and attachment IDs.

Completion generation and single acceptance are owned by
[Completion Menu E-06](completion-menu.md#acceptance-criteria).

Behavioral evidence lives in the composer, composer_workflows and composer_host
integration tests; gallery tests verify all named scenarios and fixed geometry.
Owned Unix PTY tests verify input decoding and terminal restoration. Engineering
checks and native human visual acceptance are distinct. Consumer rollout,
native Linux/Windows terminals, clipboard formats, IME composition and real
agent/shell integration require their owning host's acceptance.

Executed results are recorded in [Verification](../verification.md#composer-engineering-record--2026-10-10).

## Remaining gaps

Consumer execution, clipboard formats, durable storage and native Linux/Windows
or IME integration require host acceptance. The complete Composer human review
is tracked in [Gate 3a](../human-testing.md#gate-3a--submission-lifecycle); recorded
engineering results do not imply that gate passed. Standalone completion is
tracked in the [Completion Menu spec](completion-menu.md#remaining-gaps).
