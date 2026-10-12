# Tree Browser

Status: reusable implementation and 66 live gallery scenarios are complete.
The [engineering record](../verification.md#tree-browser-engineering-record--2026-10-12)
reports executed checks. This contract owns a reusable tree for files,
diff resources and parent/child tasks. Hosts own data, placement, execution and
the main inspection view. Engineering and human acceptance are separate.

## Model and lifetime

The caller supplies a whole ordered `Snapshot`: revision, title/summary, root
loading state, nodes and optional next-page cursor. Nodes have stable IDs,
parents, material revisions, names, file/directory/task kind, supplied status,
detail/progress text, child state, and offered actions. Parent outcomes and all
counts come from the host; the component never infers task success or Git facts.
Input order determines sibling order even when children precede their parent.

Duplicate IDs, cycles, missing parents and children beneath leaves are skipped
with a visible count. Other malformed descriptors or resource overflow reject
the whole update without changing state. Limits: 4,096 nodes, 128-byte IDs,
256 ancestry levels, 4 KiB names/query, 64 KiB metadata/reasons, 8 MiB snapshots,
64 actions per node and 256 simultaneous requests. Material changes to identity,
parent, name, kind, child capability or actions require a higher node revision.
No filesystem, Git, process, network, clipboard or terminal acquisition occurs.

## Navigation and filtering

Up/Down and Ctrl-P/Ctrl-N move through visible selectable rows; Home/End and
PageUp/PageDown use the measured viewport. Right opens a branch, then enters its
first child on a subsequent press. Left closes, then climbs on a subsequent
press; a leaf climbs immediately. Space toggles without moving. `*` expands the
selected subtree, requesting unloaded children as results arrive; `-` clears
that subtree's folds and expansion cascade. Collapsing a selected descendant
repairs selection to its parent. Fold state and cursor survive refresh by ID.
Removal climbs the old ancestry to the nearest surviving visible ancestor,
otherwise the first row, with a reason. A reparented node never remains hidden.

Slash focuses a single-line Editor. Loaded literal case-insensitive name matching
keeps ancestors and opens matching paths virtually, without changing saved folds
or discovering unloaded children. Paste only edits the query. Escape clears the
query and restores folds; selection stays on its match if visible, otherwise its
nearest visible ancestor. A subsequent Escape leaves the tree. Empty matches have
no row action. Full names remain reachable below the effective keys.

Paging counts physical rows, including loading/error/empty information, then
chooses the nearest selectable row in the travel direction. A manual collapse
stops any overlapping bulk expansion plan; delivered child loads still cache
their results without reopening the branch.

## Discovery and effects

Opening an unloaded branch emits one opaque ticketed load request. Loading/empty
rows are descriptive; failures and next-page rows are selectable. Enter retries
only the addressed branch/root or loads its next page; no automatic retries.
Paging preserves the auxiliary row while pending and when more pages remain.
Closing a branch retains its delivered load; a late valid result caches children
without reopening it. Closing the component invalidates every ticket and
undrained request. A root error is retried through the same contract.

Enter emits the host's offered primary Open or Select action. `a` uses Choice
Picker for supplied actions; disabled choices retain reasons. Supplied plain
shortcut characters dispatch the same action only outside filtering. One request
per node and one root request can be pending; other nodes remain usable.
At the pending limit, undiscovered branches remain explicitly not loaded and can
be loaded once a slot is free. Only delivered requests increment attempt counts;
capacity notices clear when capacity returns. Removed nodes release bookkeeping.
Repeat/release/modified Enter and pasted shortcuts never execute an effect.
Opaque tickets identify exact ID, material revision, action and page cursor.
Foreign, stale, duplicate and wrong-kind results are inert. No timeout is guessed.
Failure preserves authoritative data/open mark, displays its full reason and
allows explicit retry. Only an acknowledged Open/Select moves the open mark and
emits `Opened`; generic actions update through a newer whole host snapshot.
Newer snapshots invalidate removed/materially changed requests and menus.
An independent refresh of a branch's child state/page cursor invalidates its old
discovery ticket; any root refresh invalidates the old root request. Closing a
branch alone preserves its delivered load. Failed root/branch loads remain
explicitly retryable; an outstanding operation never becomes an inferred success.

## Placement and rendering

`render` draws in caller-owned areas; caller focus disables tree input and changes
the cursor from a band to an outline. Tab emits main-view focus transfer in panel
placement. Escape emits return in full-screen placement or focus transfer in a
panel, preserving the caller's Chat. The opt-in Chat Ctrl-X F entry emits a host
request, using the same entry as a host palette command.

The gallery composes a 34-column panel at 100+ columns, 24 columns at 60–99,
and a full-screen tree below 60. This is composition policy, not an internal
terminal resize operation. At three rows the selected row and keys remain;
smaller areas show the title. Names middle-elide by grapheme while retaining
extensions where they fit. Two-column guides fold every five levels with an ellipsis marker.
Status counts shorten to the status letter before consuming name space.
Selected full names and reasons are wrapped in an inspection area; Alt-PageUp/
Alt-PageDown scroll its full text when the caller provides enough height.
Open nodes use accent or underline in NO_COLOR. ASCII substitutes all tree/task
chrome; status letters and words remain meaningful without color or motion.
Clocks are caller-injected; pending age updates with static reduced-motion glyphs.

## Ownership and acceptance

`tree_browser` owns validated data, folds, filtering, cursor, tickets and rendering;
the gallery adapter supplies deterministic files/tasks and acknowledged results.
The file main view composes the separate
[Code/Diff Inspection](code-diff-inspection.md) capability; task output retains
its supplied Chat fixture.

- **TB-01:** Ordered stable navigation, folding, filtered ancestors, refresh and
  removal preserve a valid visible cursor.
- **TB-02:** Lazy/root/page loading, empty/failure/retry, collapse/late results and
  expansion cascades are bounded and reject stale or foreign replies.
- **TB-03:** Primary/custom actions and disabled reasons are host-supplied;
  pending/acknowledged/refused results never perform implicit effects.
- **TB-04:** Invalid topology is skipped safely; invalid material is atomic;
  limits and arbitrary Unicode/depth are safe.
- **TB-05:** Panel/full-screen/tiny/profile rendering, main-view focus and Chat
  return preserve caller state, with live gallery and native PTY evidence.

Human visual and broader native/platform acceptance remain pending until recorded
in [Verification](../verification.md). Consumer compilation is not integration.
