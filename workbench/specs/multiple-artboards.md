# Multiple artboards per canvas

Support-system implementation in the working tree, 2026-10-04.
The current scope is infrastructure only. The existing Workbench UI, controls,
styles, navigation, and default `index.html` entry point remain in place.
UI proposals elsewhere in this spec are future design work, not shipping behavior.
The [module plan](multiple-artboards-code-plan.md) records the implementation.

## Implemented support system — 2026-10-04

TypeScript modules under `src/canvas/` provide serializable artboard descriptors,
validated dimensions, insertion-order geometry, a controller with injected
runtime and snapshot ports, authenticated iframe messaging, full-canvas review,
and participating-space context publication. No replacement canvas page,
top bar, catalog, camera UI, artboard picker, or styles are installed.

Each instance has a stable ID, its space/view/size, a render generation, and
isolated runtime ownership. Shell and annotation ports expose the existing
renderer; closing/replacing an instance disposes its iframe and pending
requests. The controller suppresses old-generation view reports and
coordinates capture locks.
The model permits 32 instances with dimensions of 320–8192 CSS pixels, subject
to the page's authored `sizes`. Geometry includes a 48px row gap; a future host
can use pure bounds/fit/zoom functions without changing the current camera.

Review APIs compose every supplied artboard into one JPEG at CSS-pixel dimensions,
with labels, spacing, and a 32px margin. Limits are 32 megapixels and 32767px per
edge. Loading/error instances receive placeholders; ready-view capture failures
fail the complete operation. Identity is checked before and after acquisition.
Native surfaces remain live, so each region records its acquisition timestamp.
The version-2 handoff formatter names every space, region, view, status, and
local annotation coordinates; the existing single-view formatter remains
supported.

The version-2 context validator and publisher include all supplied instances.
Publication is serialized across participating spaces and their shared-root
siblings, preserving user-activity timestamps and withdrawing closed participation.
The existing UI continues to publish its single-view context. The registered-space
coordinator permits only config/context operations. Native producers are pooled
by source and codec, preserving subscribed peers; authored Simulator mappings
resolve booted names or UDIDs without requiring a catalog.

These APIs have no layout persistence or shared configuration schema. The temporary
browser acceptance harness drives the support through injected ports, not through
production controls. Verification covers instance/state/size isolation, two-space
context, combined image geometry, handoff persistence, and disposal, plus the Node
suite and strict TypeScript check. Actual Screen Recording/WDA behavior requires
the appropriate host/devices; no performance benchmark has been performed.

## UI and integration scope

No UI changes are authorized for this implementation. Adding user-facing artboard
controls or wiring these support APIs into the existing canvas requires a separate
UI assignment. The future workflows and acceptance criteria below remain design
proposals; free positioning and row/grid arrangement remain phase 2. Persistence
and sharing policy remain open.

## Purpose

Review related views together: one page at laptop and mobile sizes,
several states of a component, a design beside its implementation, or several
pages in a flow. An artboard is an instance of a view, not a new page in
the space's catalog. Two instances may point to the same page and state.
Developers and designers decide what belongs together; these examples do not
restrict the tool to a particular comparison workflow.

## Agreed requirements and phases

- **Phase 1: flexible content.** An artboard can contain any supported content
  type: authored pages, Workbench previews, URL lenses, Storybook stories,
  Simulator streams, and window streams. A canvas may combine content from
  different spaces. The model must not privilege a particular use case or
  renderer. Docs pages are included: beside other artboards a docs page is an
  artboard, as [docs pages](docs-pages.md#in-a-multiple-artboard-canvas)
  specifies.
- **Phase 1: complete review output.** When a canvas has multiple artboards,
  screenshots, handoffs, and agent context include all of them. Selection
  identifies the user's current focus within the canvas; it must not hide
  neighboring artboards from review output or context.
- **Phase 2: arrangement tools.** Support free positioning and row/grid
  arrangement. Phase 1 can use deterministic automatic placement; adding and
  reviewing multiple artboards must not depend on manual arrangement tools.
- **Unresolved: persistence and sharing.** Remembering an arrangement locally
  and storing a named arrangement in a shared project file are distinct
  options. Neither storage policy has been agreed.

## Investigation findings before implementation

These findings record the source inspected before implementation, not current
support-system gaps or a performance measurement. The implementation record
above describes the support now in place. Existing unrelated working-tree
changes were present during investigation.

- [Core](core.md) defines one selected page and one artboard. Its address
  encodes source, state, width, and lens. In
  [workbench.js](../workbench/workbench.js), those values, the rendered view,
  pending load, and frame references are canvas-wide variables. Top bar,
  sidebar, and iframe messages route through that selection.
- [zoom.js](../workbench/zoom.js) transforms and fits one `artboard` element.
  Multiple artboards need a common canvas coordinate system and bounds across
  all artboards. The current minimum zoom and single-artboard pan bounds may
  prevent fitting a large arrangement; they must be reconsidered.
- [preview-sessions.js](../workbench/preview-sessions.js) retains one active
  session and up to three inactive sessions for 15 minutes. Session keys in
  `workbench.js` use the lens and URL; Storybook groups sessions by lens and
  Storybook base URL. These identities cannot distinguish two copies of the
  same view, or two independently displayed stories from one Storybook.
- [preview-controls.js](../workbench/preview-controls.js) finds one
  `iframe.is-active`. Message handlers in `workbench.js` likewise accept
  navigation and keyboard commands from one frame. Selection and rendering
  activity need separate identities.
- [annotations.js](../workbench/annotations.js) owns one annotation list and capture
  source. Changing source or state clears annotations; changing width or lens
  preserves them for comparison. Capture dimensions come from one artboard.
  Multiple artboards require explicit annotation and capture ownership.
- [Interactive capture](capture.md) mirrors a live document into an inert
  Electron surface. Capturing a canvas composed of several live views needs
  additional composition behavior; it is not equivalent to capturing the
  selected artboard. Background mirroring also needs a deliberate budget.
- [reference.js](../workbench/reference.js) and
  [agent-view.js](../agent-view.js) report one view. The editor sidebar uses
  `wb-go` and `wb-here` for one selection. Full-canvas context needs an
  expanded report while preserving a selected-artboard field for focus.
- [Docs pages](docs-pages.md) shown alone use the docs canvas mode, and are
  ordinary artboards beside others
  ([contract](docs-pages.md#in-a-multiple-artboard-canvas)). Space switching
  currently changes the server address, so the agreed cross-space scope needs
  space-aware routing and resource ownership within one canvas.
- [Native streams](implementations.md#native-window-stream) have a verified
  implementation gap: [window-stream.js](../window-stream.js) has one current
  source/codec per manager; starting another stops the previous stream.
  Concurrent artboards need independent subscriptions and concurrent source
  ownership. Window lenses are view-only; Simulator input requires WDA. Two
  artboards showing the same device/window share that external source's state.
- [server.js](../server.js) refuses mutations from another space's origin.
  A canvas served by space A cannot simply send existing capture, input,
  or source-opening requests to space B. A space-aware host transport is
  necessary; broadening the routes to arbitrary origins is not a solution.
- [spaces.js](../spaces.js) identifies a space by configuration directory
  and optional manifest key, not just its served root. Several spaces can
  share a root and still have different configurations. Runtime space IDs
  depend on local paths and are not portable team-sharing references.
- [Shield's context reader](../../../shield/src/integrations/workbench-context.mjs)
  validates the reporting server's root and consumes `view.text`.
  [Agent context](agent-context.md#finding-the-server) also notes that spaces
  sharing a root share one announcement file. Multi-space publication must
  account for this, and context transport needs changes beyond the canvas UI.

## Suggested phase 1 interactions

- Start with the existing single-artboard experience. **Add artboard** opens
  a page/state choice; a sidebar action can add a page directly.
  Ordinary sidebar picks replace the selected artboard's view. Add is an
  explicit action so normal navigation does not accumulate artboards.
- **Duplicate artboard** copies page, state, lens, and dimensions into a
  fresh preview instance. Runtime form edits, scroll, and application state
  are not cloned. Control overrides and annotations need a decided copy
  policy before implementation; the recommended default is a fresh instance.
- Each artboard has its own page, state/story, lens, size and dimensions,
  controls, actions, loading/error state, and annotations. Its descriptor
  includes space identity and source/server resolution. Different spaces,
  content types, and repeated instances can coexist on the same canvas.
- Exactly one artboard is selected. Its outline and label identify it; the
  existing top bar, sidebar highlight, source, and reload act on it.
  Copy reference, screenshot, handoff, and agent context describe the entire
  canvas and identify the selected artboard. Provide a keyboard-accessible
  artboard list so selection and closing do not require a pointer. An empty
  canvas has no selection and disables view-dependent actions.
- Place added artboards in a tidy row automatically in phase 1. Avoid
  rearrangement during preview edits. Phase 2 adds free movement by label and
  explicit row/grid **Arrange** actions; dragging inside the preview keeps
  interacting with its content.
- Zoom and pan apply to the whole arrangement. Provide **Fit all** and
  **Fit selected**. Resizing an artboard preserves preview CSS dimensions
  independently of canvas zoom. Define the distinction between the current
  Fit size and fitting the camera before implementation; freeze Fit
  to explicit dimensions when adding a comparison artboard is one option.
- Interacting with a visible preview selects its artboard and reaches its
  content. Navigation originating inside a preview replaces that artboard's
  view, leaving its neighbors intact. Cross-origin focus and input selection
  require rendered verification rather than assuming parent pointer events.
- A failed or loading artboard does not disable its neighbors. Reload is
  scoped to the selected artboard. Closing disposes that artboard's resources
  and selects a predictable neighbor; closing the last returns to an empty
  canvas.
- Keep annotations local to each artboard. Selection changes preserve them;
  replacing its page/state clears them, while lens and size changes follow
  the existing comparison behavior. Undo targets the selected artboard.
- Screenshots capture all artboards, including ones outside the current camera
  viewport, with their labels and annotations. Capture geometry is independent
  of camera zoom. Suggested output: a composite image retaining their relative
  positions, plus per-artboard images in a handoff for legibility. Exact output
  format and large-canvas handling remain open.
- Handoffs include every artboard's space, source, state/story, lens, size,
  code pointers, and annotations, mapped to the corresponding captured image
  region or image. Keep stable IDs so repeated views remain distinguishable.
- Agent context reports the whole canvas through the existing agent-context
  capability: an artboard list, each artboard's space and view details,
  selected ID, and loading or failure status. One unavailable artboard must
  not make all context disappear. This must reach existing hooks and MCP
  readers, not just the canvas UI.
- If restoration is included, use versioned descriptors carrying every
  artboard's space identity. Restore views and arrangement rather than
  runtime application state. Browser and editor canvases should not silently
  overwrite one another's arrangements. The persistence destination is open.
- Preserve existing single-view URLs. An explicit page
  link selects that view; define how it interacts with any restored layout.
  Full-layout links and shared named canvases need their own contract if
  included. Single-artboard agent context retains existing view information;
  multi-artboard context extends it with the whole canvas.
- Docs pages in artboards follow
  [their contract](docs-pages.md#in-a-multiple-artboard-canvas): 1056 × 900 by
  default and resizable, scrolling inside the artboard, the canvas's zoom, and
  per-artboard lens, state, and example.

## Proposed system and edge-case contracts

These are review recommendations, not additional agreed product decisions.
They make the implementation boundaries concrete without narrowing content.

### Canvas and artboard ownership

- A canvas owns its ordered artboard list, selected ID, camera, automatic
  placement, and capture operations. Each artboard owns its view descriptor,
  render generation, loading/error state, annotations, and renderer lifecycle.
  Content adapters expose mount/update, readiness, capture, and disposal.
  Capabilities such as controls and input differ by content type; the shell
  must not assume every artboard has an iframe, DOM, controls, or input.
- Distinguish the requested view from the settled view. An outgoing preview
  may remain visible while replacement loads, but its image must not be
  labelled as the new page. Capture and context carry the actual rendered
  identity plus any pending request. Failed replacement preserves the usable
  outgoing view and names the failed target; a first-load failure has a named
  placeholder and Retry.
- Duplicate creates a fresh renderer and copies only space/view/size
  descriptors, with default controls and no copied annotations. Independent
  iframe state does not imply independent cookies, storage, backend state,
  or native device/window state. Never clone or reset an external device to
  simulate independence.
- Closing selects the next artboard in order, or the previous one if there
  is no next. Closing the last leaves an empty canvas. Canvas shortcuts respect
  editable fields and preview keyboard behavior. Selection by keyboard or
  pointer does not remount content or consume its first intended interaction.
- In phase 1, use stable insertion order and a fixed-gap automatic row in
  canvas coordinates. Resizing can move following artboards to avoid overlap,
  but does not remount them or reset the camera. Fit all operates on bounds;
  Fit selected is unavailable when empty. Exact spacing is a UI design detail.
- Suggested size policy: retain existing Fit behavior for one artboard;
  on adding a second, resolve Fit into its current explicit CSS dimensions.
  Multi-artboard sizes remain independent of canvas-window resizing. Returning
  to one artboard does not implicitly change its dimensions. Native content
  retains its renderer's aspect ratio and coordinate mapping.

### Spaces and host transport

- Keep one server/preview worker per space. A canvas coordinator uses the
  existing space registry and space-aware host adapters to resolve requests;
  artboard identity includes space ID and manifest location/key. Ephemeral
  server URLs are resolved at runtime, not treated as durable source identity.
- Choosing a space in the catalog changes which pages can be browsed;
  it preserves existing artboards. Selecting an artboard synchronizes the
  catalog and top bar with its owning space. Adding from a different
  space is explicit; ordinary picks replace the selected instance.
- Route configuration edits, source opening, preview navigation, capture, and
  native input to the owning space. Accept iframe messages only from the
  registered frame/source, expected origin, and current render generation.
  Space transport preserves current path validation, configured source
  restrictions, and host trust rules. Do not open arbitrary filesystem roots
  or permit arbitrary cross-origin mutations to make mixed spaces work.
- Removing a space or losing its server leaves its artboards as named
  unavailable placeholders with Retry and Close; other spaces stay usable.
  Configuration refresh reconciles only artboards owned by that space.
  Missing source/state/lens produces a named problem rather than silently
  changing the reviewed view. Shared servers and stream producers stop only
  when their owner and remaining subscribers no longer need them.
- Native producers may be shared by source and codec with separate artboard
  subscriptions. Closing one subscriber cannot stop another. Different sources
  and codecs run concurrently. Simulator input targets its device with its
  current coordinate mapping; window artboards remain view-only. Existing
  platform/prerequisite limitations appear per artboard, not as a content ban.

### Capture and handoff

- At action start, freeze artboard membership, order, positions, dimensions,
  actual rendered descriptors, and annotations. Acquire each live snapshot or
  native frame before releasing its renderer. Adding, closing, resizing, or
  navigating afterward cannot relabel or substitute a captured artboard.
  A resource lost before snapshot acquisition fails that capture explicitly.
- Flush each live renderer and compose frozen snapshots; never re-run project
  scripts to reconstruct a comparison. Native frames and independent renderers
  are sampled during one operation, not guaranteed to represent one instant.
  Record per-artboard acquisition timestamps. Preserve the existing capture
  fidelity/unsupported-element contracts and label any URL fallback that
  represents a separate browser session.
- Suggested first-release output: one composite JPEG download using canvas
  bounds, background, labels, and annotations, excluding the top bar, toolbar,
  selection chrome, and camera cropping. Artboard coordinates use CSS pixels;
  any output scale must be explicit and uniform. Choose image-size and memory
  limits before implementation; never silently crop, omit, or reduce quality.
- Suggested failure policy: loading/error/unavailable artboards appear as named
  status placeholders in the complete canvas image and handoff. An unexpected
  capture failure on a ready artboard fails the operation with its ID and reason;
  no successful handoff is emitted with that artboard silently absent. Empty
  canvases cannot be captured or handed off. Cancellation releases temporary
  resources and does not leave an apparently complete bundle.
- A handoff is one canvas review with stable artboard IDs, space-qualified
  source/code paths, and annotation coordinates local to each artboard,
  plus their composite image rectangles. Native targets without DOM elements are
  identified as such. Persist one bundle in a defined host space/location;
  multi-space relative paths alone are ambiguous. Its destination and optional
  per-artboard images are unresolved product choices. Catalog-wide
  [design-system export](export.md) keeps its own capture plan.

### Agent context and compatibility

- Publish a versioned canvas record with canvas ID, artboard order, selected
  ID, per-artboard space/view/size/status, and complete readable `text`.
  Keep selected-view compatibility fields for existing consumers, but existing
  hooks and MCP must consume the complete canvas text rather than only those
  fields. Match Copy reference to that same record.
- Suggested discovery: publish the full canvas record to each participating
  space's context service, under the same canvas ID. Each service keeps its
  own root for Shield validation. Handle shared-root announcements explicitly
  rather than allowing space selection to decide which context survives.
  Closing or removing participation withdraws the record; heartbeat expiry
  still handles vanished canvases.
- Publish on add/remove, view/status/size changes, and selection changes.
  Selection and user changes determine the most recently used canvas; automatic
  loads and heartbeats must not steal precedence from a different canvas the
  user has subsequently used. Context stays available for partial loading and
  failure, and makes no claim that an agent has access to every referenced root.
- Every artboard must survive server and reader size limits. Establish a bounded
  format and explicit oversize error or complete retrieval mechanism; slicing
  text and silently dropping later artboards violates the agreed requirement.
- Suggested legacy-link behavior: an explicit single-view URL opens a fresh
  one-artboard canvas; it does not merge into or overwrite a restored comparison.
  Browser Back/Forward routes within its owning artboard rather than appending
  artboards. Full-layout links and restoration history await a persistence
  decision. A zero-artboard canvas reports open with an empty list and no focus;
  closing the canvas reports closed.

## Implementation plan

1. **Agree the interaction contract.** Resolve the questions below and create
   a rendered prototype using Acme fixtures: laptop/mobile, two states, and
   design/implementation, native streams, and mixed spaces. Verify selection,
   adding, closing, automatic placement, and keyboard access in browser and
   VS Code. Manual positioning and row/grid controls belong to phase 2.
   Use the shared canvas/artboard [terminology](terminology.md) and coordinate with
   the existing docs modules. Agree capture limits, failure/output policy,
   host space ownership, and context discovery before implementation.
2. **Extract artboard ownership while preserving one-artboard behavior.**
   Introduce capability modules under `src/` for artboard descriptors,
   selection, commands, and preview lifecycle. Keep DOM, host messages, and
   persistence as explicit adapters. Give each instance a stable ID, space
   owner, and its own pending-load sequence. Resolve catalog, source, origin,
   server readiness, and native stream/input resources against that owner.
   One space's refresh or server failure must not reset other spaces.
   Bound this extraction to the affected workflow.
3. **Render multiple artboards and add canvas geometry.** Use one world
   transform, positions in CSS-pixel coordinates, shared bounds, and separate
   selection. Adapt preview routing and controls to instance IDs. Visible
   previews must not expire as inactive sessions merely because another
   artboard is selected. Retained hidden sessions need a canvas-wide budget
   rather than multiplying the current allowance by artboard count.
4. **Integrate full-canvas review and recovery.** Bind annotations and
   mirror/stream subscriptions to their artboard instances. Compose capture across all
   renderers and spaces, and extend references, handoffs, and agent-context
   reports to carry every artboard and the selected ID. The server's existing
   text limit and single-space agent discovery require explicit compatibility
   work; reports must not silently drop artboards. Snapshot artboard membership,
   descriptors, and annotations when capture starts; selection changes must
   not redirect it. Define errors and temporal consistency when several live
   renderers are captured. If persistence is agreed, version and validate its
   descriptors. Handle removed catalog entries visibly, and release resources
   on closing, refresh, and space changes.
5. **Validate and document.** Test instance isolation, superseded loads,
   geometry, navigation, capture identity, persistence, and disposal with the
   existing Node runner. Run the package tests and TypeScript check. Exercise
   real browser and VS Code rendering for iframe focus, shortcuts, resizing,
   cross-origin lenses, native streams, mixed-space lifecycle, and capture.
   Verify full-canvas context through agent hooks and MCP readers.
   Measure multi-artboard memory and input
   responsiveness before selecting limits. Update core and affected specs,
   public guides, website copy, and screenshots with the shipped behavior.
   If saved canvases change YAML, update all schema references required by the
   package instructions in that same implementation change.
6. **Phase 2: arrangement.** Add free positioning and row/grid arrangement
   with accessible movement controls. Preserve artboard identity, runtime
   state, annotations, and full-canvas review output while positions change.

## Proposed acceptance criteria

- Laptop and mobile instances of the same preview remain visible together;
  typing, controls, scroll, and reload in one do not alter the other instance.
  Frames provide independent document state; shared origin storage, cookies,
  and a live backend can still be shared and should not be claimed isolated.
- Two Storybook stories from one implementation stay independently selected
  and rendered. Switching one cannot switch or evict the other.
- Selecting an artboard updates all selection-dependent controls and the
  focused ID in full-canvas context without remounting its neighbors.
  A stale frame message or load cannot
  change another artboard or steal selection.
- Fit all includes every artboard; Fit selected centers the selected one.
  Canvas zoom does not change viewport layout or screenshot dimensions.
- Notes stay with their owning artboard. Screenshots and handoffs include all
  artboards and their annotations, including off-camera artboards. Capturing
  while selection changes retains the originally requested canvas membership
  and view identities. Capture failures name the affected artboard and never
  silently omit it from an apparently complete result.
- Agent context includes every artboard, its space and view, and the selected
  ID through both hooks and MCP. A loading or failed artboard is represented
  without hiding usable neighbors.
- Authored pages, Workbench previews, URL lenses, Storybook, Simulator, and
  window content coexist in phase 1, including content from different spaces.
  Source opening, navigation, reload, capture, and handoff resolve to each
  artboard's owning space. Simulator input reaches the intended device;
  window lenses remain view-only. Two artboards on one native source share its
  live state without replacing each other's stream subscriptions.
- Closing or refreshing disposes owned listeners, timers, adapters, capture
  subscriptions, and streams. A hidden-session limit cannot evict a visible
  artboard. Rendered interaction remains usable at the agreed artboard limit.
- A legacy page link still opens its named view. If layout restoration is
  included, one missing space or source does not discard all valid artboards.
- Phase 1 adds artboards with predictable automatic placement. Phase 2 allows
  free positioning and row/grid arrangement without remounting previews.
- Existing one-artboard workflows pass in both browser and editor hosts.
- A space refresh, removal, or failed start cannot reset another space's
  artboards. Two spaces sharing a root remain distinguishable by manifest
  identity. Space-aware transport rejects stale/wrong-origin messages and
  unconfigured source requests.
- A captured replacement never receives the outgoing view's pixels under its
  new label. Closing or navigating during capture follows the snapshot/failure
  contract, and every image region maps to one stable handoff artboard ID.
- Complete context reaches agents working in every participating configured
  space; shared-root spaces and simultaneous editor/browser canvases do
  not overwrite one another's identities. Heartbeats and background readiness
  do not change which canvas the user last used.
- Verify oversize capture/context behavior and cleanup at the chosen limits;
  measurement targets and limits are release criteria still to be established.

## Questions for product decisions

1. **Saving and sharing:** Should Workbench remember an arrangement only on
   the user's machine, or also save named arrangements in a project file that
   teammates can commit and reopen? For example, a shared "Checkout review"
   could name its artboards and views. The destination and portable references
   for a canvas spanning several spaces need decisions if sharing is wanted.
2. **Capture format:** Is one composite canvas image sufficient, or should
   handoffs also include a full-resolution image of each artboard? All artboards
   are included either way. Large arrangements need a defined resolution and
   image-size policy.
3. **Docs integration:** decided 2026-10-04 in
   [docs pages](docs-pages.md#in-a-multiple-artboard-canvas).
4. **Canvas ownership:** Where should a mixed-space handoff be saved, and
   which space owns canvas-level actions such as export and configuration?
   Recommendation: a host space owns the handoff bundle; editing and export
   explicitly identify the space they target. Participant sources retain
   their own space identities regardless of the selected artboard.

## Documentation delivery

Public guides describe supported behavior. Apply the following changes when
the corresponding implementation is verified; do not publish planned controls,
YAML keys, shared-layout behavior, or capture formats as available features.

| Source | Verified release behavior to document |
| --- | --- |
| `docs/README.md`, package README, website overview | Flexible multi-artboard canvas across supported content/spaces; full-canvas review output |
| `docs/canvas.md` | Add/duplicate/close/select, automatic placement, per-artboard editing, Fit all/selected, navigation and empty/error states; phase 2 arrangement only when shipped |
| `docs/annotations-and-handoff.md` | Artboard-owned annotations; all-artboard captures including off-camera content; image labels/dimensions, failures, bundle location, space-qualified paths, clearing annotations and concurrency |
| `docs/spaces.md`, `docs/extension.md` | Browsing versus canvas ownership, space-qualified source/config actions, unavailable-space recovery, context from all participants and shared-root handling |
| `docs/workbench-previews.md`, `docs/preview-data.md`, `docs/storybook.md` | Instance-scoped controls/actions/navigation and independent stories/previews; external storage/backend sharing |
| `docs/ios-simulator.md`, `docs/windows.md` | Concurrent subscriptions, same-source live state, device-specific input, view-only windows and per-artboard prerequisites |
| `docs/configuration.md` | Only any actually adopted/implemented saved-canvas schema; update all shared schema references in the same change |
| `docs/design-system-export.md` | Explicit space target; catalog reference export remains distinct from canvas review |

The public terminology and single-artboard scope can be clarified before the
feature ships. Maintain existing routes and update website navigation/search
when adding a guide. Run website tests/build, inspect rendered guide content,
and refresh overview screenshots only when shipped visible behavior changes.

## Review status

The implementation record above supersedes the investigation's proposed defaults
where it gives concrete choices. Persistence/sharing remain unagreed, and manual
arrangement is phase 2. The investigation and target design below remain useful
for future migration and performance work; they are not claims of completed
package-wide migration.
