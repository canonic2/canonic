# Browsing, panels and notices

Status: session/resource browsers, trees and general content panels remain
proposed. Reusable [Dialog](../components/dialog.md),
[Option Picker](../components/option-picker.md) and
[Command Palette](../components/command-palette.md) have their own component contracts.

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
[Conversation](../components/chat.md).

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

Use changing remote queries, delayed deletion failures, task/file trees, narrow
sidebars, unknown context cost, notice expiry and clipboard failure. Fixtures
must not load real sessions/files/models or contact services.

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
