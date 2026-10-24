# Session Browser

Status: reusable implementation and 72 live gallery scenarios are complete.
[Verification](../verification.md#session-browser) maps its test coverage. This public contract owns the dedicated screen,
independent of session storage and transport. Human visual/platform acceptance
remains separate from engineering verification.

## Purpose and composition

Navigate supplied conversations, resume one, and request rename, fork, move,
pin/unpin or delete. A caller owns the terminal and all Chat instances. Opening
the browser hides Chat without replacing, cloning or serializing its Composer,
transcript, selection, logical anchor, unread state, history or queued messages.
Escape returns to the originating Chat. A successful open acknowledgement returns
the destination ID; only the caller changes the displayed Chat instance.

The browser is a dedicated screen, not an overlay over Chat. It accepts events,
elapsed time and caller-owned frames; it performs no filesystem, clipboard,
network, session storage, execution or notification I/O. The `sessions` palette
command and Chat's opt-in Ctrl-X O both request this same host-owned view.

## Host data and boundaries

`Snapshot` supplies a monotonically increasing revision, scope, ordered rows,
load state, empty wording, remote-search capability and optional paging cursor.
Each `Session` has a stable ID and material revision, title, project/ref, group,
optional parent, pinned flag, last activity, state text/tone, age, preview and
loaded message-search text. The originating ID determines the current marker.
Groups and ordering belong to the host; the browser never guesses recency, dates,
status, project ownership or pinned placement. One fork level is drawn; missing
parents are allowed for paged/filtered lists, but supplied cycles are rejected.

Each row supplies its offered actions and disabled reasons. Absent actions are
absent from menus/hints. Move destinations are host-supplied picker choices.
Rename supplies its Unicode scalar character limit (default 80); Delete requires exact host
confirmation wording describing the loss. A current session may be deleted only
when the host explicitly offers it. The host rechecks every request's authority.

Validate complete snapshots before adopting them. Reject duplicate/empty/control
IDs, duplicate action kinds/destinations, invalid limits/parents, and over-budget
material atomically. Bounds: 4,096 rows; 128-byte IDs; 256 move destinations per
row; 4 KiB query/rename editors; 64 KiB preview/message-search text per row;
8 MiB total snapshot material; 4 KiB titles; 64 KiB full deletion wording;
256 pending row requests across visible and hidden results. Render safe control markers from retained source.
Resource-overflow field insertions are rejected whole. Rename character overflow
keeps all typed text and disables save, with a visible count/reason.

## Navigation and search

Opening selects the originating row when loaded, otherwise the first row.
Up/Down move between selectable rows; groups/errors are not cursor positions.
PageUp/PageDown use the measured viewport; Home/End reach first/last loaded row.
The cursor follows IDs through insertion, reordering, pinning and paging. Removal
selects the next surviving row in the previous visible order, then the previous
one, then the first new row or empty state; show a reason. Never retain a stale ID.

Slash focuses an Editor query except on a ready, genuinely empty list. Loaded
search is case-insensitive literal matching against titles and supplied message
text. Title matches precede message-only matches; each partition keeps host order.
Message-only matches are identified as such. A final `Search all` row is available
only for nonempty queries when the host enables remote search. Enter on a normal
result opens it; Enter on the final row emits a search request. Typing/paste only
edits the query and cannot invoke a row action. Up/Down navigate while the field
has focus; letters remain query input until Escape leaves the field.

Remote search retains usable loaded matches while pending. Results carry the
exact ticket/query generation and host-ordered snapshot. A query edit, clear,
scope refresh, close or newer search invalidates the older ticket and any
undrained intent. Stale/foreign/duplicate results are inert. Query changes return
to loaded filtering. Search failure retains loaded results and exposes an
explicit retry through the Search all row. Escape first stops a pending remote
search, then leaves the query field, then clears the filter, then returns to Chat.

At the loaded or remote-results end, navigation emits at most one identified `More` request for
the host paging cursor. Pending paging preserves cursor/list. Failure shows the
reason and requires R to retry; no automatic retry loop. A successful whole
snapshot completion preserves selection by ID. Remote paging carries its query and paging cursor, and query edits invalidate it.
Initial loading is a host-supplied status; R retries failed discovery.
Loading supports search before
rows arrive; empty states have no row actions. Load errors retain earlier rows
and offer explicit R retry. All clocks are injected; there is no automatic timeout.

## Row actions and acknowledgement

Enter opens/resumes an enabled row. Enter on the originating row simply returns
without host execution. `a` opens a Choice Picker under the row; shortcuts are
R rename, F fork, M move, P pin/unpin and D delete. Only offered, enabled actions
run; disabled actions remain readable in the menu. Move uses an independent
Choice Picker with disabled destinations/reasons. Menu/destination choice is
deliberate; paste never executes an action.

Rename embeds Editor in the title cell with the complete old title selected.
Enter saves valid single-line text; Escape cancels. Ctrl-J cannot insert a newline
into a single-line name; pasted newlines are refused whole. A pending rename
shows the typed title, dims the row and labels `renaming`. On failure/refusal,
restore authoritative row material and retain the complete typed field. Reopen
it immediately if that row remains selected and no other local interaction is
open; otherwise R on that row restores the failed draft without stealing focus.

Fork, move and pin emit requests, without inventing a new row/order/project.
Actual successful snapshots supply the result; Fork also supplies the new ID and
moves selection to it. Pin/unpin follows the row ID in the host's new order.
Other action failures retain the row and show the host reason; the same shortcut
starts a fresh request. Open failure stays in the browser and leaves Chat intact.

Title, project, parent, pin state, actions, destinations and rename-limit changes
require a higher material row revision; older or contradictory revisions are
rejected atomically. Activity, age, state and preview updates may keep that revision.
Filtering or remote results hiding a row do not cancel its pending operation.
Returning to loaded filtering retains newer known material for loaded rows.

Each request contains row ID, material revision, typed operation payload and an
opaque ticket. A row has at most one pending action; other rows remain usable.
Pending rows display the operation verb and cannot submit twice. Open requests
are also single-flight across the browser to avoid competing session switches.
Acknowledgements consume exact tickets once. Results must fit the requested
operation and include a valid higher-revision authoritative snapshot when rows
change. Invalid results preserve the pending operation for correction. Material
row replacement/removal invalidates that row's request and local editor/menu/
confirmation; unrelated row refreshes preserve pending requests. Undrained
invalidated intents are filtered before delivery. Closing invalidates all tickets;
it cannot undo an operation already delivered to the host.

## Delete confirmation

D opens a shared Dialog with full host wording, Keep selected and Delete never
preselected. Digits only highlight; Enter on Keep cancels. Enter on Delete arms
for three injected seconds; the second unmodified Enter sends once. Movement,
paste, timeout, replacement or cancellation disarms. Escape keeps the session.
Long confirmation text wraps and scrolls with PageUp/PageDown; no loss wording
is silently cut. Pending delete keeps/dims the row and permits navigation away.
Only the authoritative successful snapshot removes it. Failure restores the row
and reason; D repeats the complete confirmation. Last deletion yields empty.

## Rendering and profiles

Title rule shows scope/count and offscreen counts. Groups are dim; selected row
uses the package cursor band, with an ASCII `>` marker where applicable. Current
uses `●`/`*`; forks use `└`/`` `- ``. State text always accompanies semantic tone.
Rows under 50 columns use title/age then project/state on a second line. At 50–99
columns show title/project/state/age; at 100+ add last activity and a read-only
supplied preview when height permits. Short screens hide preview first. Long
titles use grapheme-safe ellipsis; rename fields scroll instead of truncating data.
All rows, errors, menus and confirmations remain reachable with tiny areas.

Footer prioritizes Escape and the current Enter action, then search/actions;
wide views add only offered enabled shortcuts. NO_COLOR uses reverse selection,
bold errors and explicit pending verbs; ASCII changes decorations independently
of color, and light uses the same geometry. Reduced motion uses static pending
marks with elapsed seconds, never loses state. Preserve the existing package
ANSI16 cursor policy; this component does not settle the broader open decision.

## Implementation ownership and acceptance

`session_browser` owns descriptors/validation, navigation/query and operation
lifetimes; Editor owns field editing, Choice Picker owns action/destination
menus, Dialog/Confirmation own deletion layering and arming. Gallery hosts retain
separate Chat instances and simulate data/actions without real session effects.

- **SB-01:** stable selection, groups/current/forks, paging/removal repair, bounds.
- **SB-02:** local/remote search, explicit retries and query/load/refresh races.
- **SB-03:** all offered actions, disabled reasons, concurrent pending rows,
  exact single-use tickets, material races and failed rename draft recovery.
- **SB-04:** safe host-worded deletion, timed disarming, success/failure/last row.
- **SB-05:** dedicated screen and exact per-session Chat/Composer restoration,
  background unread updates, explicit open failure and successful switching.
- **SB-06:** meaningful live scenarios, tiny/narrow/profile rendering, behavioral
  and native PTY checks, public docs, consumer compilation and recorded evidence.

Human visual acceptance, wider platform coverage and real consumer storage,
discovery/action adapters are separate checkpoints. Tree Browser and other
roadmap components are outside this implementation scope.
