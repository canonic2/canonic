# Multiple artboards per canvas

A scenario's declared artboards appear together inside the canvas as artboard
instances; the existing sidebar, top bar, toolbar and view controls operate on
the selected instance. This topic owns the requirements and acceptance of
artboard instances and declared groups. Artboard management (adding,
duplicating, closing, Fit operations, arrangement and its persistence) is a
separate proposal in [artboard management](artboard-management.md). The
[artboard system](artboard-system.md) owns the module architecture.

**Status:** implemented: the instance model, isolation, full-canvas review
composition and context publication, and the canvas showing one scenario's
declared artboards together. Not implemented: user-facing multi-artboard
acceptance across mixed content and spaces, and a capture memory budget; see
[implementation gaps](#implementation-gaps). Management workflows are a
[proposal](artboard-management.md).

Required architecture: [one shell with content-only renderers](../system/rendering-flow.md).
The outer application entry point remains unchanged; it must never become an
artboard's content entry point. Instances use the dedicated `content.html` viewport.

## Instance model and review limits

`ui/src/bootstrap/canvas.js` composes the artboard modules under
`ui/src/modules/` through `wb-canvas` inside the shipping canvas region. No
replacement top bar, sidebar, toolbar, artboard picker or inspector is installed.

Each instance has a stable ID, its space/view/size, a render generation, and
isolated runtime ownership. Explicit content/annotation ports expose metadata,
navigation and capture without hidden application controls. Closing/replacing an instance disposes its iframe and pending
requests. The controller suppresses old-generation view reports and
coordinates capture locks.
The model permits 32 instances with dimensions of 1–8192 CSS pixels. A declared
artboard opens at its authored width and height; an instance of an implicit
artboard follows the page's sizes and the size switcher, under
[sizes](sizes.md#multiple-artboards).
Automatic placement wraps at 1600 logical pixels
with 48px gaps. Selection preserves live inputs and annotations. An origin-aligned
`data-wb-fit-content` root opts into natural content bounds; an owned ResizeObserver
tracks edits. Content fitting pauses during capture locks. Transparent corners
use the dark review surface when flattened into a JPEG.

Review APIs compose every supplied artboard into one JPEG at CSS-pixel dimensions,
with labels, spacing, and a 32px margin. Limits are 32 megapixels and 32767px per
edge. Loading/error instances receive placeholders; ready-view capture failures
fail the complete operation. Identity is checked before and after acquisition.
Native surfaces remain live, so each region records its acquisition timestamp.
The version-2 handoff formatter names every space, region, view, status, and
local annotation coordinates; the existing single-view formatter remains
supported.

The version-2 context validator and publisher include all supplied instances,
and the shipping canvas publishes its complete version-2 context, as
[agent context](../integrations/agent-context.md#requirement-the-canvas-context-record)
specifies. The registered-space
coordinator permits only config/context operations. Native producers are pooled
by source and codec, preserving subscribed peers; authored Simulator mappings
resolve booted names or UDIDs without requiring a catalog.

There is no layout persistence or shared configuration schema for instances.

## UI and integration scope

The shipping integration
opens one scenario's explicitly declared artboards together. Scenarios and Storybook
stories remain alternatives. Each scenario's `artboards` map declares stable
IDs, labels, dimensions, input overrides and iterations
([artboards and iterations](../pages/iterations.md)); the renderer receives
`context.artboard`. Artboard inputs apply after scenario inputs. Artboards are
declared only inside a scenario; a scenario that declares none has one implicit
artboard. The Acme demo's Alert component declares five artboards in its
default scenario. A page selected in a docs lens shows one reading viewport, not its
declared artboards; docs placed beside other artboards are an artboard, as
[docs](../pages/docs-pages.md#in-a-multiple-artboard-canvas) specifies.
Ordinary single-view pages reuse one renderer across navigation, as
[navigation](navigation.md#navigation-feedback) specifies. The required
content-only renderer owns the retained preview sessions and Storybook
channel. Explicit artboard groups
own independent renderers. While a group is visible, at most one ordinary
renderer stays attached, hidden and inert, for up to 15 minutes. It is excluded
from canvas state, selection, review captures and context publication, and
cannot select a visible artboard or send editor navigation messages. Returning
restores that renderer and its retained sessions; clearing the canvas or closing
the host disposes it. Expiry waits for any capture to finish before disposing it.
Management workflows, free positioning, row/grid arrangement, persistence and
sharing are [proposals](artboard-management.md).

## Purpose

Review related views together: one page at laptop and mobile sizes,
several scenarios of a component, a design beside its implementation, or several
pages in a flow. An artboard instance on the canvas shows one view: one
declared or implicit artboard of a page and scenario. It is not a new page in
the space's catalog, and its instance ID is separate from the declared artboard
ID. Two instances may point to the same page and scenario.
Developers and designers decide what belongs together; these examples do not
restrict the tool to a particular comparison workflow.

## Agreed requirements

- **Flexible content, in the first delivery.** An artboard can contain any supported content
  type: authored pages, Workbench previews, URL lenses, Storybook stories,
  Simulator streams, and window streams. A canvas may combine content from
  different spaces. The model must not privilege a particular use case or
  renderer. Docs lenses are included: beside other artboards a page's docs are
  an artboard, as [docs](../pages/docs-pages.md#in-a-multiple-artboard-canvas)
  specifies, so one page's design and docs can sit side by side.
- **Complete review output, in the first delivery.** When a canvas has multiple artboards,
  screenshots, handoffs, and agent context include all of them. Selection
  identifies the user's current focus within the canvas; it must not hide
  neighboring artboards from review output or context.
- Arrangement tools, agreed for a later delivery, and arrangement persistence and sharing are in
  [artboard management](artboard-management.md#agreed-and-open).

## System and edge-case contracts

The [instance model](#instance-model-and-review-limits) establishes the
baseline for instance identity, geometry, capture and context. Proposals for management interactions
are in [artboard management](artboard-management.md#proposed-edge-case-contracts);
they do not reopen these limits.

### Canvas and artboard ownership

- A canvas owns its ordered artboard list, selected ID, automatic
  placement, and capture operations; its camera follows
  [interaction](interaction.md#camera-and-scrolling). Each artboard owns its view descriptor,
  render generation, loading/error state, annotations, and renderer lifecycle.
  Content adapters expose mount/update, readiness, capture, and disposal.
  An embedded renderer fills its owning artboard at 100% width and height. Its
  content viewport contains no application shell, canvas grid or inner frame shadow, including after a docs/lens
  transition; background and layout authored by preview content remain its own.
  Capabilities such as controls and input differ by content type; the shell
  must not assume every artboard has an iframe, DOM, controls, or input.
- Requested and settled views, the grace period, capture while pending and a
  failed replacement in one artboard follow [navigation](navigation.md).
- Selection by keyboard or pointer does not remount content or consume its
  first intended interaction. Canvas shortcuts respect editable fields and
  preview keyboard behavior, under [interaction](interaction.md#focus-and-command-ownership).
- Placement uses stable insertion order and a fixed-gap automatic row in
  canvas coordinates.
- Size policy is owned by [sizes](sizes.md#multiple-artboards).

### Spaces and host transport

- Keep one server, with its preview compilers and Node evaluator, per space. A canvas coordinator uses the
  existing space registry and space-aware host adapters to resolve requests;
  artboard identity includes space ID and manifest location/key. Ephemeral
  server URLs are resolved at runtime, not treated as durable source identity.
- Selecting an artboard synchronizes the catalog and top bar with its owning
  space.
- Route configuration edits, source opening, preview navigation, capture, and
  native input to the owning space. Accept iframe messages only from the
  registered frame/source, expected origin, and current render generation.
  Space transport preserves current path validation, configured source
  restrictions, and host trust rules. Do not open arbitrary filesystem roots
  or permit arbitrary cross-origin mutations to make mixed spaces work.
- Removing a space or losing its server leaves its artboards as named
  unavailable placeholders with Retry and Close; other spaces stay usable.
  Configuration refresh reconciles only artboards owned by that space.
  Missing source/scenario/lens produces a named problem rather than silently
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
- Review produces one composite JPEG using canvas
  bounds, background, labels, and annotations, excluding the top bar, toolbar,
  selection chrome, and camera cropping. Artboard coordinates use CSS pixels;
  any output scale must be explicit and uniform. The image bound is 32
  megapixels and 32767px per edge. A memory budget must be defined before UI
  delivery; never silently crop, omit or reduce quality.
- The capture policy represents loading/error/unavailable artboards as named
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
  per-artboard images are unresolved
  [product choices](artboard-management.md#questions-for-product-decisions). Catalog-wide
  [design-system export](../review/export.md) keeps its own capture plan.

### Agent context and compatibility

[Agent context](../integrations/agent-context.md#requirement-the-canvas-context-record)
owns the versioned canvas record, its compatibility fields and its 32-artboard,
100,000-character bound;
[publication, precedence and heartbeats](../integrations/agent-context.md#publication-precedence-and-heartbeats)
owns participating-space and shared-root publication, withdrawal and which
canvas is current.

## Acceptance criteria

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
- Canvas zoom does not change viewport layout or screenshot dimensions.
- Notes stay with their owning artboard. Screenshots and handoffs include all
  artboards and their annotations, including off-camera artboards. Capturing
  while selection changes retains the originally requested canvas membership
  and view identities. Capture failures name the affected artboard and never
  silently omit it from an apparently complete result.
- Agent context includes every artboard, its space and view, and the selected
  ID through both hooks and MCP. A loading or failed artboard is represented
  without hiding usable neighbors.
- Authored pages, Workbench previews, URL lenses, Storybook, Simulator, and
  window content coexist in the first delivery, including content from different spaces.
  Source opening, navigation, reload, capture, and handoff resolve to each
  artboard's owning space. Simulator input reaches the intended device;
  window lenses remain view-only. Two artboards on one native source share its
  live state without replacing each other's stream subscriptions.
- Closing or refreshing disposes owned listeners, timers, adapters, capture
  subscriptions, and streams. A hidden-session limit cannot evict a visible
  artboard.
- A single-view page link still opens its named view.
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
  not overwrite one another's identities, and precedence follows
  [agent context](../integrations/agent-context.md#publication-precedence-and-heartbeats).
- Verify oversize capture/context behavior and cleanup at the implemented
  limits.

## Verification

- Unit and workflow tests live with the modules:
  [canvas tests](../../ui/src/modules/canvas/canvas.test.js),
  [publication tests](../../ui/src/modules/canvas/publication.test.js),
  [review capture](../../ui/src/modules/review/capture.test.js) and
  [review controller](../../ui/src/modules/review/controller.test.js) tests,
  [preview-session tests](../../ui/src/modules/preview-sessions/controller.test.js)
  and [canvas element tests](../../ui/src/components/canvas/element.test.js).
- [smoke-canvas.cjs](../../scripts/smoke-canvas.cjs) drives real Chrome and the
  packaged capture runtime through a programmatic harness with injected ports:
  instance, state and size isolation, two-space context, combined image
  geometry, handoff persistence and disposal.
- [smoke-canvas-ui.cjs](../../scripts/smoke-canvas-ui.cjs) drives the production
  shell's existing controls and a scenario's declared artboards, and checks that
  ordinary HTML and Button scenarios remain single alternative views, with
  input, size and annotation isolation, preview controls and composite native
  handoff.
- Not covered: actual native capture and Simulator input on their hosts and
  devices, the editor host, and performance.

## Implementation gaps

- User-facing multi-artboard acceptance above (mixed content types and spaces,
  including docs beside other artboards, per-space transport and placeholders,
  oversize capture/context behavior) is not established; the remaining work is
  the [delivery plan](artboard-management.md#delivery-plan).
- The capture memory budget beyond the image bound is undefined.
