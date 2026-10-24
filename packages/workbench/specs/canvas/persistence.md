# Persistence and restoration

This topic owns what Workbench remembers, its scope, and how a canvas opens
after a reload or restart. [Core](../core.md#selection-and-navigation) owns routing;
[VS Code](../integrations/vscode-extension.md) owns editor integration. These requirements are
agreed for implementation; [current status](#current-status) distinguishes the
existing storage from the required restoration behavior.

## Opening a canvas

Once the current space's complete page list is available, resolve the initial
view in this order:

1. An explicit destination, such as a sidebar pick or an addressed page link.
2. That space's last saved selection, if its page still exists.
3. The first page in the resolved collection/group/page order, with that page's
   default scenario, lens and size under the existing routing rules.

The ordinary single-view canvas must not wait for a click when usable pages
exist. Choose from the same ordered, merged page model as the sidebar, including
discovered and imported pages; skip empty collections/groups. Do not select
from a partial list or wait for example compilation or content readiness to
choose a destination. The selected page can then show its normal loading or
failure state. An empty space retains its existing empty/problem presentation.

An explicit invalid address follows the routing/error contract; do not conceal
it by opening a saved or first page. Only an absent destination invokes startup
restoration. A newer user pick always supersedes storage reads, config loads and
pending restoration. Config refresh preserves the current valid destination;
it does not reapply startup restoration over the user's navigation.

## What is saved

| State | Scope and lifetime |
| --- | --- |
| Current space | Local to the VS Code workspace/window context; restore it if still available, otherwise use the first listed space |
| Last selection | One record per workspace and stable space identity: page source, scenario/story, selected iterations (the address's `+` parts, per declared or implicit artboard), lens key, size and explicit Docs example anchor where applicable |
| Viewer preferences | Actions mode, sidebar width, preferred lens/size keys and custom resizable dimensions; local to the viewer, reused across its spaces and validated against the current configuration |
| Added spaces | Existing editor-wide local list; separate from each workspace's current space and page selections |

Selection records store logical view data, not server URLs, proxy addresses,
ports, runtime IDs or display labels. Preserve custom dimensions when they are
part of the selected size. Resolve defaults and normalize the restored address
against current configuration, using the same rules as ordinary navigation.
The sidebar's selected collection/group/page follows the restored page; a stale
search filter must not prevent the restored selection or first-page fallback.
Persisting filters and expanded groups is outside this change.

Save the latest accepted destination after routing has resolved its page and
view parameters. Content need not render successfully: a selected Dev lens whose
service is stopped is still a valid selection, and restoring it shows its
normal lens failure. Never save an unvalidated request, a superseded completion,
or temporary null selection during initialization, space switching or teardown.
Pending navigation must not overwrite a later accepted selection.

## VS Code ownership and lifecycle

- The extension owns durable state through `ExtensionContext.workspaceState`
  for the current space and per-space selections. `globalState` continues to
  own the added-space list. Keep this local; do not opt selection records into
  Settings Sync or write them to project files.
- Editor viewer preferences also use workspace-owned host storage, shared by
  its spaces. Restore them before applying the selected view; explicit view
  parameters win over preferred lens/size keys. Their survival must not depend
  on the embedded server's browser origin.
- Identify a space by its configuration location and stable space key/root,
  including the workspace/remote authority. Two spaces, separate workspace
  contexts or remote hosts must not collide. A changed localhost port or
  extension installation path does not change the space identity. Renamed or
  moved spaces need no heuristic migration; an unmatched space starts fresh.
- The canvas owns navigation and reports its normalized destination over the
  existing validated host relay. The extension stores the complete view,
  including lens, size and example; sidebar highlighting alone is insufficient.
  Scope messages and writes to their owning space and panel generation.
- On **Open Canvas**, restored panel startup, **Developer: Reload Window** or
  editor restart, start the current space's server and restore the logical
  destination against its current config. Never retain a former server URL.
  An explicit pick during startup wins over the saved destination.
- Switching spaces saves each accepted selection independently and restores
  the destination last used in the destination space. An unvisited space opens
  its first page. Returning to a space must not erase the other space's record.
- Closing the Workbench tab keeps the selection for the next explicit open.
  It must not reopen the tab. VS Code decides whether a tab is restored with
  its workspace; registration of a webview serializer must not create a tab
  that the user closed. Activation alone follows the existing editor opening
  rules. `retainContextWhenHidden` preserves an open tab's live session and is
  separate from durable restart restoration.

## Browser ownership

Use native IndexedDB for browser selections and viewer preferences, without
a database library, wrapper dependency or network service. VS Code uses its
host's `workspaceState` instead; the embedded browser does not maintain a
second durable copy of editor state. Both hosts expose the same logical
read/write contract to navigation while owning their storage lifecycle.

Browser and editor viewers follow the same destination precedence and first-page
fallback. They maintain independent local selections; opening a browser link
does not overwrite the editor's selection. Browser records use the stable space
identity rather than a port, tab ID or ephemeral proxy origin.

IndexedDB is origin-bound: a change of scheme, hostname or port accesses a
different database, even when the records use a stable space key. Browser
restoration is guaranteed only while the serving origin is unchanged and the
browser retains its data. A new origin starts with the explicit destination
or first-page fallback. Cross-origin migration is outside this change; stable
keys prevent spaces on the same origin from colliding, not origins from
isolating their storage. An explicit URL works without any stored state.

Version the database schema and records. Keep transactions short and persist
small logical records only. Close database connections on version changes;
blocked upgrades, denied access, quota failures, eviction and private-session
storage limits follow the best-effort fallback below. A failed open or upgrade
must settle without leaving startup waiting indefinitely. Do not require
durable-storage permission or automatic destructive database resets.

## Validation, failure and bounds

- Use a versioned, validated record. Ignore corrupt, unsupported or invalid
  records and use the first-page fallback without blocking the canvas.
- A deleted saved page falls back to the first page. A removed scenario, lens,
  size or example on an existing page follows current routing defaults; keep
  that page and persist the normalized result. An unavailable but still
  configured lens remains selected and shows its failure on the canvas.
- Storage reads/writes are best-effort. Failure must not prevent navigation,
  readiness or first-page selection. Coalesce writes while preserving the
  latest accepted destination; observe errors without unhandled rejections.
  Normal close/reload must flush queued writes as the host lifecycle permits;
  after abrupt process termination the last completed write is the guarantee.
- Keep one selection per known space, not a navigation history. Remove its
  record when an added space is explicitly removed. Validate records when
  workspace configuration changes and discard records for spaces no longer
  present. Read/write only the owning viewer's state; concurrent workspace
  contexts do not follow one another's live navigation.

## Concurrent viewers

Native storage does not automatically isolate windows. Tabs on one origin can
share IndexedDB; editor windows for the same workspace can address the same
workspaceState keys. Stable space identity prevents space collisions, not
conflicting writes from two viewers of that space.

Each live viewer keeps its own accepted destination. A storage notification or
another viewer's write must not navigate it or override a newer local pick.
The latest-write fencing above applies within its owning lifecycle.

The choice of which viewer's saved selection wins on the next open is unresolved:
one shared last-completed record or separate identified-viewer records are
possible policies. Define that conflict policy before claiming independent
per-window restoration. Existing workspaceState use does not establish it,
and no cross-tab synchronization or compare-and-swap API is implied here.
Include simultaneous tabs/windows, late writes and close/reopen in restoration
acceptance once the policy is selected.

## Session-only state and exclusions

Retained frames preserve form values, menus, component state, open code panels
and scroll while their live documents survive, under
[core retention](../core.md#canvas-and-artboard) and the
[warm Docs pool](../preview-pipeline/warm-runtime.md#warm-docs-pool). A restart
mounts fresh content at its normal initial position, or an explicit example
anchor. Do not serialize page DOM, form contents, component instances, timers,
workers, capture mirrors, annotations, zoom/pan or arbitrary project inputs as
part of selection persistence. Existing saved capture/handoff artifacts retain
their own lifecycle.

Multiple-artboard arrangements, camera restoration, named/shared canvases and
cross-device synchronization remain outside this contract; see
[multiple artboards](multiple-artboards.md#agreed-requirements-and-phases).
Startup fallback must not undo a deliberate clear/close-last-artboard action
within an active comparison session.

## Acceptance

- A fresh workspace opens its first page after listing; no empty-canvas click
  is required. Empty spaces and failed catalogs retain their normal errors.
- Reloading/restarting VS Code or closing/reopening Workbench restores the
  last page, scenario/story, selected iterations, lens, size/custom dimensions
  and example anchor.
  Page runtime state mounts fresh; a hidden live tab keeps its existing state.
- Switching A → B → A restores each space independently. Two workspace/remote
  contexts do not overwrite one another. Restarting on a different server port
  or after reinstalling the extension preserves the editor selection.
- An explicit pick/link wins over saved state, including a pick arriving while
  restoration is pending. Late acknowledgements cannot overwrite it.
- Deleted pages, changed configuration, invalid records and unavailable storage
  yield the documented fallback. A configured stopped lens restores its canvas
  message without a sidebar availability warning.
- Closing a tab or window while startup is pending never reopens it. The
  sidebar and canvas agree after restoration and after config refresh.
- Browser reload/reopen on the same origin restores from IndexedDB. Spaces on
  that origin keep independent selections. A different origin or cleared/denied
  storage falls back normally; database open/upgrade failures do not block
  navigation. Embedded VS Code persistence works without IndexedDB access.

Add focused navigation/storage and panel lifecycle tests, then verify normal
close/reopen, **Developer: Reload Window**, full editor restart and port changes
in installed VS Code. Existing navigation tests do not establish these new
restart guarantees. Browser persistence needs separate host evidence.

## Current status

The extension saves the current space in `workspaceState` and added spaces in
`globalState`. Viewer preferences currently use browser `localStorage`, scoped
to the served origin. The canvas URL holds the current view; the panel keeps
only in-memory selection and uses `retainContextWhenHidden`. These do not
implement durable complete-selection restoration or stable preference storage
across port changes. A canvas without an explicit destination opens the first
page from the resolved collections, including on space switches; imported
implementation-only pages open their first named scenario. Config refresh does not
reapply this fallback.

Implementation must add a scoped complete-selection record, extend the host
relay, restore the panel through the editor lifecycle, and add saved-selection
restoration before the first-page fallback. Browser mode must add native IndexedDB for
selection and preference records; editor preferences must move to `workspaceState`.
Cross-origin browser migration is excluded and arrangement persistence remains
unresolved.
