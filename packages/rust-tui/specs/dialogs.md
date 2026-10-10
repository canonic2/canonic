# Dialogs, commands and browsing

Status: rich picker implemented and human accepted in slice 2a. Dialogs,
commands, sessions, trees and panels below remain proposed targets.

## Dialog composition

Dialog host owns an explicit stack, target ID, size policy, body component,
title, close policy and return focus. Only topmost layer handles input. Resize
clamps to available dimensions; body scrolls and active action stays reachable.
Tiny screens may use the full available area with abbreviated decoration.
Closing discards only that layer's transient state; parent form/draft survives.
Outside click is opt-in dismissal and must not confirm or activate background.

Confirmation, text prompt, choice, alert/help and inspection dialogs reuse
`review`, `editor`, `picker` and `content`. Bodies neither start loops nor call
global dialog services. Stack replacement/clear reports dismissal for each owned
interaction and invalidates its asynchronous generation. No callback survives a
destroyed dialog unnoticed. Local failure remains in the dialog with corrective
host action when available, rather than unexpectedly exiting the entire screen.

## Picker contract

`ChoicePicker` accepts `Choice` values with stable ID, label, detail, group,
disabled reason, danger and current flags. Duplicate IDs reject construction or
refresh atomically. Default selection uses a supplied enabled non-danger ID,
otherwise the first safe row. Filter changes preserve a surviving enabled ID,
otherwise use the first safe ranked match. Refresh retains valid enabled marks
and selection by ID. Host-supplied `Status` distinguishes ready/loading/error;
empty source and no matches have separate messages. No retries or loading run.

`Filter::Auto` enables filtering above seven choices; On/Off override it. The
bounded 4 KiB query uses the editor's grapheme selection, paste and undo. Matching
is a lowercased subsequence, ignoring query whitespace. Label matches rank before
detail matches; score sums matched grapheme positions, with source order breaking
ties. Whole matched graphemes are bold/underlined. This local algorithm owns no
remote query generation or locale/normalization service.

Arrows/page/Home/End move among enabled rows and keep highlight visible, including
group headers and single-line choice rows. Disabled rows remain explanatory but
cannot be chosen. Danger rows are never implicit defaults. F3 returns `Inspect`
without confirming. Multi-selection is a separate
explicit mode with selected set and final submit; ordinary single-choice Enter
must not change meaning because a consumer supplies many rows. Returned IDs refer
to supplied data, never filtered positions. Compatibility adapters can retain
current original-index results for static choices: the original `Picker`/`Action`
API remains unchanged. Space toggles a mark; Alt-A toggles matching enabled rows.
Hidden marks survive the query and final `Confirmed` IDs follow source order.
With a nonempty source an empty marked set can confirm. Plain Enter press is the
only final confirmation; repeated/modified Enter, paste and disabled shortcuts
are inert. Esc clears a nonempty query before returning `Cancelled`.

`PickerView` supplies title/source/context (up to three rows), maximum height
(default nine) and presentation. Height derives from source content at opening
and stays stable while filtering, clamped by host area. Details disappear before
labels truncate at grapheme boundaries; group rows count toward paging. Hosts
own placement and execute/recheck returned intents. This supports stage/dock/
full-screen gallery placement; an inline terminal adapter remains slice 12.

## Commands, completion and help

Host supplies command ID, label/category, aliases/optional slash spelling,
description, availability/disabled reason and effective binding. Palette searches
and returns an invocation ID; it performs no dispatch, shell execution or parsing
of agent commands. Help derives from the same descriptors/bindings, with category
and pending-sequence hints. Current context controls visibility, not a persistent
global registry. Availability is rechecked by host after selection.

Slash completion and command palette share descriptors while using separate
insertion/invocation contracts. Accepting a completion can insert command text or
choose an explicit command intent according to its descriptor; it cannot also
send a chat message. Custom skills/agent commands remain host-provided rows.
Model, provider, agent, permission mode, variant and theme selectors reuse the
picker/browser; discovery, credentials and changing backend settings are host work.

## Session and resource browsers

Session browser accepts stable rows, title, safe metadata, current/recent/pinned
flags, parent ID, loading/error state and available row actions. Local filtering
and supplied remote-query intents are separate modes. Query generation prevents
late search results replacing newer ones. Current selection survives refresh by
ID; deletion/removal moves predictably to another permitted row or empty state.

Select, rename, delete, fork, move and resume return typed intents. Destructive
actions use host-supplied confirmation and stay pending until acknowledged;
application error restores a usable list. Pinned/recent ordering and search scope
are host-supplied. UI does not create sessions or adopt an agent's storage format.
Timeline browser reuses rows but its message IDs and jump behavior belong to
[Conversation](conversation.md).

Trees accept explicit nodes/parent relationships and child-load capability.
Expand/collapse/navigation uses logical node IDs; unloaded children show loading
or retry. Removing a focused descendant returns focus to surviving ancestor.
Reject cycles/duplicate IDs safely. Keyboard can move to parent/first child and
select without filesystem reads. File/diff trees and parent/child task trees use
the same node contract; node metadata and actual operations remain host-specific.

## Status panels and notices

Optional sidebar sections show host facts: context usage, supplied costs, todos,
changed files, server/LSP/MCP status and child activity. Each has explicit unknown,
loading, ready, empty or error state. UI never calculates vendor pricing or probes
connections. Narrow layout hides optional sections before draft/review actions;
hidden sections remain accessible through an inspection dialog.

Notices contain stable ID, severity, label/details, dismissal/action and optional
expiry on an explicit clock. Errors requiring action remain reachable after a
toast expires. Reduced motion removes pulse/spinner movement while retaining
status text. Attention requests are visible indicators or returned host intents;
OS notifications, audio, debug-log reading and telemetry are outside the library.

## Gallery fixtures and acceptance

Use nested dialogs, removed return focus, long/disabled/grouped rows, no matches,
changing remote query, delayed deletion failure, task/file trees, narrow sidebar,
unknown context cost, notice expiry and clipboard failure. No fixtures load real
sessions/files/models or contact services.

- **D-01:** Dialog stack handles only top input, restores valid focus, cancels
  owned generations and remains operable at tiny sizes and after resize.
- **D-02:** Picker filters/pages variable rows by stable identity; disabled/empty/
  loading/error states cannot confirm; original-index compatibility is verified.
- **D-03:** Palette/help/completion agree on enabled descriptors/bindings and emit
  exactly one intended action without execution or background leakage.
- **D-04:** Session action/search/refresh/delete races preserve identity, reject
  stale results and expose pending/error; no persistence is performed.
- **D-05:** Tree loading/removal/cycle handling remains safe and keyboard reachable,
  returning only node intents without filesystem/agent access.
- **D-06:** Optional sections/notices degrade at narrow width, expose unknown/error
  and deterministic expiry, and require no hidden clock or notification service.

## Open decisions

Responsive panel thresholds and
whether session navigation has a dedicated screen or dialog need gallery review.
Keep host-provided command names out of generic public contracts.
