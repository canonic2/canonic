# Dialogs, commands and browsing

Status: proposed target. Current picker/catalog provides simple single-choice
search; reusable overlays, rich rows, command and session views are gaps.

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

Richer choices have stable ID, label, description, category, optional secondary
metadata, disabled reason and selected/current marker. Safe caller default or
none is preserved. Filter supports deterministic substring initially; later fuzzy
ranking must be documented with stable ties and highlighted matches. Changing
query preserves surviving choice ID; otherwise highlights a permitted result
according to explicit filter policy. Disabled results can remain visible for
explanation but cannot confirm. Loading, empty source, no matches and failure
are distinguishable.

Arrows/page/Home/End move among enabled rows and keep highlight visible, including
variable-height grouped rows. Search has grapheme editing, paste and a bounded
query. Rich rows may open detail without confirming. Multi-selection is a separate
explicit mode with selected set and final submit; ordinary single-choice Enter
must not change meaning because a consumer supplies many rows. Returned IDs refer
to supplied data, never filtered positions. Compatibility adapters can retain
current original-index results for static choices.

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

Fuzzy matching algorithm, row grouping defaults, responsive panel thresholds and
whether session navigation has a dedicated screen or dialog need gallery review.
Keep host-provided command names out of generic public contracts.
