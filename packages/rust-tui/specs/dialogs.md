# Dialogs, commands and browsing

Status: rich picker human accepted in slice 2a. Dialog stack mechanics and slice
2b.1 visual design are implemented, with visual human acceptance pending. Slice
2b.2 typing-aware arrival and timed removal notices remain unimplemented.
Commands and keys are implemented in slice 2c, awaiting their human gate.
Sessions, trees and general content panels remain proposed.

## Official dialog design

The user-supplied revised contract is authoritative: one centered panel in every
view. [Dialog design](../design/dialog/dialog.html) and its
[PDF](../design/dialog/dialog.pdf) supply anatomy and behavior reference;
the revised placement contract below governs implementation.
It contains Overview, Keys & behavior and States boards. The implementation below
covers the visual shell and stack mechanics; it does not establish human acceptance
or implement the timing requirements below.

- Every view uses a centered panel, at most 72 columns and viewport height minus
  four rows. Its lighter surface has one column of padding on each side.
- Chat hosts open an alternate screen, redraw the visible conversation tail there,
  and return on close; scrollback stays untouched. This transition is pending the
  inline terminal adapter. The full-screen gallery already owns an alternate screen.
- Height follows content. Prompt fields grow from one to six rows before scrolling.
  No padding rows to a fixed preferred height.
- Shared chrome contains a task title, optional scope at right, an optional
  one-to-three-row message, content and a footer with host-supplied action words.
  Editors use terminal-background insets inside the filled surface.
- Only the top dialog is visible. A nested opening replaces the parent visually
  and shows a title trail; Escape resumes the preserved parent. Replacement adds
  no trail entry. Clear returns to the original background focus.
- Visible background rows repaint faded, including the composer, while drafts
  and cursor state survive. Dark RGB fading uses `#5E6064`, distinct from ordinary
  secondary text; indexed mode uses color 240 while live grey uses 245.
  The visible activity row remains live: warning-colored dialog
  status and normal grey hints identify Escape behavior or nesting depth. A tall
  panel may cover it; the activity row never paints over dialog content.
- An arrival within one second of typing waits instead of taking focus. The
  activity row advertises `! dialog waiting · ctrl+g`; Ctrl-G opens it explicitly.
  Timing must be an explicit host input, not a hidden component clock.
- Host removal restores focus and displays the reason for three seconds on the
  explicit host clock. Ctrl-C remains an application action in every state.
- ANSI 16 and no-color modes use a full box and the editor's own field rules;
  indexed/RGB modes use the filled surface. Narrow layouts wrap messages, drop
  scope metadata and shorten footer text while keeping keys reachable.
  Box title/footer lines leave a space next to each corner. Boxed prompt scroll
  hints belong only to the editor's line. In no-color the dialog title is bold
  only; reverse emphasis belongs to the focused field title. The background is
  left plain. ANSI 16 live hints use default foreground plus faint.

Design fixtures are prompt, multiline prompt, danger confirmation, notice,
nested, replaced, waiting, removed, cleared, scrolling, narrow and ANSI 16.
The gallery header, fixture controls and background test editors are host UI.
The gallery has eleven fixtures covering visual presentation and stack mechanics.
Waiting and timed removal fixtures belong to 2b.2. Existing stack/focus/ticket
tests remain engineering evidence.

## Dialog composition

Implemented `dialog::Stack<B,T>` owns up to sixteen caller-supplied bodies and
wraps instance-local `Focus<T>`. `Dialog` supplies a unique open ID, title, body
target order, preferred cell size and close policy. Duplicate IDs/targets or depth
overflow reject push/replace before changing existing bodies/scopes. Closed body
ownership returns in `Dismissed` with its ticket and reason; clear reports top to
bottom. Replacement reports the old layer and creates a fresh opening.

Tickets are unique per opening, including between independent stacks. Only the
active ticket permits body updates or submission closure; foreign, obscured,
closed and replaced results are rejected. Hosts can retry current data after an
obscured parent resumes. The stack starts no asynchronous work and retains no
completion callback; hosts cancel their own work from dismissal records and
clear before tearing down a composition with outstanding tasks.

Dispatch calls only the top handler. Tab/Shift-Tab advances its scope; releases
are consumed. Escape is offered to the body first so a selection/filter clears
before closing; policy may forbid escape closure. Outside pointer events are
consumed; only an opted-in outside left press dismisses. It never confirms.
The gallery simulates that press without enabling terminal mouse capture.

Every body and host adapter follows the universal
[keyboard contract](interaction.md#keyboard-contract-and-terminal-quirks).
Shift/Alt-Enter insert prompt newlines and never confirm a choice or submit a form.

`DialogView` supplies scope, message and normal/compact action footer.
`Stack::render_with` measures body height at the available width before placing
the centered panel. Panels are at most 72 columns and leave four viewport rows.
Tiny viewports omit chrome below three rows. The compatibility
`render` method retains the preferred size height without a body measurement.
Only the top body renders, with parent titles in its trail. The view fades the
already-rendered visible background, fills the surface in indexed/RGB modes,
and uses a full box in ANSI 16/no-color. Body components own scrolling and action
reachability; gallery prompt fields grow to six rows, then scroll. The eleven
gallery scenarios use an editor, picker and local inspection text.
General review/content bodies and pointer capture remain their owning later slices.

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

Slice 2c implements `commands::Commands` for palette and searchable keyboard help,
using one supplied descriptor map, command-specific columns and shared rules/tokens. Labels, categories,
descriptions, aliases, disabled reasons and effective binding text are projected
from that map. Selection returns a stable command ID; it never executes anything.
Availability must be rechecked by the host. Replace a context map atomically by
constructing a new validated view and dispatcher before swapping either instance.
Slash insertion/completion is still slice 4 work; aliases are searchable here.

Host supplies command ID, label/category, aliases/optional slash spelling,
description, availability/disabled reason and effective binding. Palette searches
and returns an invocation ID; it performs no dispatch, shell execution or parsing
of agent commands. Help derives from the same descriptors/bindings, with scope groups, source/conflict details
and delayed pending-sequence hints. Current context controls visibility, not a persistent
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
