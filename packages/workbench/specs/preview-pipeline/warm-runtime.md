# Warm runtime

Part of the [preview pipeline](README.md). This spec covers the work done
before a click so that the next page is quick: pre-warming when a project
starts, and keeping a runtime that has already loaded the shared code,
stylesheets and fonts. Frame retention and eviction are owned by
[core](../core.md#canvas-and-artboard); this spec adds what the pipeline keeps warm.

## Purpose

In an application with client-side navigation, the first page pays for
loading the framework, shared code, global stylesheets and fonts, and later
pages pay only for their own code. Previews should work the same way. A newly
selected page costs only its own chunks, its mount, and any font or image it
loads that the runtime does not have yet.

## Requirements

Warm documents contain [project content only](../system/rendering-flow.md). Retaining a
full Workbench application document is not an acceptable warm-runtime mechanism.
Keep the existing Storybook channel and project content alive without retaining
hidden application controls or another canvas.

### Pre-warming on project start

- When a trusted space with native pages starts, the Workbench server's
  compiler starts without waiting for a request. It indexes the project,
  declares its entries ([compiler](compiler.md#entries)), restores the
  persistent cache and builds. Today this happens only when the host passes
  `--eager-previews` ([status](#status)).
- A request during pre-warming waits for that build, which contains its
  entry, and never for a later one.
- The canvas prepares its warm runtime as soon as the build reports, before a
  native page is selected.
- Pre-warming never runs project code outside the Node evaluator, and never
  runs in an untrusted space or with `previews: false`.

### The warm runtime

For previews, the canvas keeps, per project and profile, a runtime that has loaded:

- the preview runtime and request mocks;
- the profile's runtime chunk and the shared chunks;
- global stylesheets from the project environment;
- the web fonts those stylesheets load.

A selection mounts into one long-lived warm document per owning content renderer,
project and profile. Independent artboards own independent mutable documents;
they may share compiled chunks and catalog data, not a document or input state.
Between pages it restores its baseline and loads the selected preview's project
modules while retaining shared dependencies, stylesheets and fonts. Scenario,
input and size changes re-render the current page without reloading its modules.
[Managed-preview lifecycle](../pages/previews.md#open-scenarios-controls-and-actions)
defines host mounting and retained import-time resources.

### Isolation in a reused document

A preview must leave nothing behind for the next one
([performance](../system/performance.md#speed-never-costs-correctness)). Between
previews, a reused document removes everything the previous preview added
after the document's baseline:

- stylesheets, including those `style-loader` inserted;
- window and document listeners;
- timers, intervals and animation frames;
- body children outside the preview root, including portals;
- attributes and classes on `<html>` and `<body>`;
- `window` globals;
- request mock handlers.

It also:

- removes the previous preview's project modules from the module cache, so
  they evaluate fresh, and keeps modules from `node_modules`. `style-loader`'s
  own modules are evaluated fresh too, so its registry starts over;
- keeps the globals that the bundler and hot runtime own: the chunk registry,
  hot-update callbacks, React Refresh and Vue hot-reload hooks, and
  Workbench's own globals. Those globals belong to the retained compiler runtime, not to a page.

Import-time body nodes keep their retention rule in
[previews](../pages/previews.md#open-scenarios-controls-and-actions).

### Warm Docs pool

The [performance contract](../system/performance.md#measured-interactions) owns
the improvement goal; [Docs](../pages/docs-pages.md#reusable-page-lifecycle)
owns page start/stop behavior. This section owns document reuse, isolation,
freshness and resource bounds between destinations.

The canvas uses a dedicated Docs host, separate from the preview host. The
Docs capability owns its browser-native host under `ui/src/modules/docs/host/`
and its reusable page lifecycle under `ui/src/modules/docs/page/`. The host is
a content-only document; neither module initializes the Workbench shell.

- Pool ownership is local to one content renderer and space. Documents are
  compatible only within the same docs lens and compiler profile/generation;
  independent artboards never share a mutable document.
- An exact retained destination resumes its existing document, mounted examples,
  actions, open code panels and scroll without a new mount or blocking request.
- For a destination that is not retained, the session owner first expires idle
  entries and lists the remaining idle frames oldest first. Once three sessions
  are idle, reuse the oldest eligible idle Docs document of the same lens.
  Active or protected loading documents are never candidates. If no compatible
  idle Docs document exists, open a new one and apply normal retention eviction.
  Reusing a candidate can differ from global oldest-first eviction when the
  oldest session belongs to another lens or content kind.
- Reassignment removes the old destination's retention key and binds the same
  iframe to the new destination. The old page is no longer a retained return.
  Protect the replacement until it is promoted or cancelled, as for other
  content frames. The session owner supplies retention/order; the content-frame
  factory selects the host kind and owns its loading and disposal.
- The host fetches Markdown HTML and the selected lens's examples metadata in
  parallel. It passes that metadata to the page lifecycle without issuing the
  same request again. Plain Markdown and unavailable examples retain their
  normal rendering and error behavior. Readiness still waits for examples,
  fonts and visible images; earlier text does not count as a ready destination.
- The host accepts only served Docs destinations in its own space/origin and
  applies the fetched content and options without executing the standalone
  page bootstrap again. Existing public page URLs, lens/scenario/example addresses
  and relative links/assets keep their meaning. Each parallel response belongs
  to the same requested page/lens/scenario; an edit during loading must converge
  to the latest revision rather than leave mismatched Markdown and examples.
- Between assignments, keep the host/page scripts, loaded shared chunks,
  applicable module stylesheets and their fonts. Stylesheets needed by the next
  module stay parsed; obsolete module stylesheets are removed after that
  module establishes its required styles. Project modules evaluate fresh on
  reassignment, using the preview compiler's existing dependency-retention rules.
- New destinations start at the top; an explicit example address takes
  precedence. Retained returns preserve scroll. A refresh of the same destination
  preserves its reading position. Scroll memory must stay bounded by retention;
  it does not retain disposed examples or grow with the navigation history.

#### Resume and freshness

`resume()` makes an already-ready retained page available for immediate
promotion, then checks its revision in the background. An unchanged revision
does no work on the page. A changed revision refreshes the same destination,
keeping scroll and the selected scenario/lens. Markdown-only changes follow the
in-place update contract, preserving mounted examples; example changes follow
the adapter's hot-update or remount rules. Push updates remain the normal edit
path; the return check catches an update missed while idle or disconnected.

Each load, refresh and check belongs to a destination and lifecycle generation.
Late responses after another assignment, cancellation, reload or disposal must
not modify content, readiness, scroll or the active canvas. A failed background
check leaves the retained page usable and permits a later push or resume to
retry; it must not turn a return into a loader. A failed changed-page refresh
uses normal page/example error reporting and remains recoverable. An incompatible
compiler generation or unsafe runtime is replaced by a fresh Docs host for the
requested view, preserving its view parameters.

#### Docs document isolation

Before assigning a different page, stop the old page and restore the host's
baseline: remove page listeners on window/document/html/body, timers,
intervals, animation frames, body children including portals, added globals,
and text-backed styles; restore html/body attributes and classes. Retain
host/compiler/library globals owned by the warm runtime. Keep CSSOM-built
library stylesheets whose owning dependency runtime keeps references, including
React Native Web's sheet. This exception does not permit page text styles or
DOM sentinels to leak across assignments.

Dependency initialization can explicitly assign root listeners to the retained
runtime. The React adapter scopes `createRoot()` through the Docs host's
`__workbenchRetainRuntimeListeners` hook, keeping React's document event
delegation and its initialization marker alive together. The scope is synchronous;
component rendering/effects remain page-owned. Retained listeners survive page
reassignment and are released on host disposal. Event names alone never decide
ownership: a page's own `selectionchange` listener is still cleaned up.

The reusable page's `start(options)` and `stop()` contract is owned by
[Docs](../pages/docs-pages.md#reusable-page-lifecycle). The host awaits cleanup before
starting the next page. Closing, expiry, Reload and configuration refresh stop
page work and dispose the document. Shared push connections remain owned by
their creator; page cleanup removes its subscription without closing another
page's connection. Cancellation also invalidates pending source, highlighting,
revision, metadata and mount work. A failed cleanup cannot allow an unisolated
document to serve the next page; replace it with a fresh host. The canvas keeps
the current requested URL and retries it when the replacement document loads,
including its lens, scenario, actions and anchor. Replacement invalidates old work
and closes its runtime subscriptions. A late cleanup failure from a cancelled
or disposed request cannot navigate a newer destination or reopen a removed frame.

### Resource bounds

- At most one preview warm document per owning content renderer, project and profile.
  Independent artboards are distinct owners. This bound does not permit a global
  mutable document shared by multiple simultaneously visible artboards.
- Retained frames keep the limits in [core](../core.md#canvas-and-artboard): one
  active and up to three inactive sessions for 15 minutes. A warm document
  counts as one session. Docs and previews use the same retention budget,
  not a separate budget per host kind or lens. The Docs pool creates no
  extra spare host outside those sessions; protected replacements follow the
  existing loading lifecycle.
- The accepted Docs cost is about 13 MB more JS heap with four documents
  (one active and three idle), based on the heavy-fixture long walk. This
  replaces the lab's requirement that the pool stay within control's heap;
  it does not authorize more retained frames or unbounded growth. See
  [measured conditions](../../research/docs-navigation.md#warm-docs-pool-decision-2026-10-23).
- Warm-document and retained-frame memory must have measured bounds. A
  document whose retained resources exceed its bound is replaced without
  losing the requested view. Choosing the threshold and safe replacement
  behavior remains implementation work.

## Status

Pre-warming is implemented in `workbench-server`'s preview compiler: once the
page tree configures the compilers, they build every listed preview and docs
lens ([compiler](compiler.md)). The warm preview document is the preview-host
document in `ui/legacy/` ([preview-host.js](../../ui/legacy/preview-host.js)),
served by the server as `/_workbench/preview-host.html`. A new scenario, input
or size of the preview a warm document shows re-renders through the preview
runtime without reloading its modules.

What starts pre-warming today: the server configures its compilers when it
first assembles the page tree. Started with `--eager-previews`, which the
extension and the standalone command both pass, it does that at startup
without waiting for a request: `Space::warm_docs` assembles the page tree,
which indexes the project and configures the compilers (so every listed
preview and docs lens starts building), then requests each docs lens's bundle
in turn ([server](../system/server.md#request-tracing)). Without the flag,
the first page-tree request starts it. Gaps:

- Pre-warming on space start depends on the host passing `--eager-previews`;
  the server alone waits for the first page-tree request. The target of
  preparing a shared browser runtime before any native selection is not
  established by server-side pre-warming alone.
- Returning to a page the warm document showed earlier loads its modules
  again rather than guaranteeing per-page runtime-state retention. The
  [retained-page requirement](../system/performance.md#navigation-keeps-the-canvas-usable)
  and resource budget must be met together.
- The warm document is not replaced after a bound; its heap after long walks
  is not limited.

The Docs pool, reusable page lifecycle and nonblocking return revision check
are implemented in `ui/src/modules/docs/`, with host-kind routing and idle
selection in `content-frames.js`/`preview-sessions.js`. The server preload
correction is implemented independently in the generated client
([serving](serving.md#preload-selection)). The selected design is the lab's
`pool-rp`; no compiler warm-up redesign is part of this change. Shipping-code
confirmation is tracked in the [Docs navigation research](../../research/docs-navigation.md#warm-docs-integration-2026-10-23).

## Verification

Isolation and warm-host lifecycle are covered by
[preview-host tests](../../ui/legacy/preview-host.test.js) and
[preview-runtime tests](../../ui/preview/preview-runtime.test.cjs). The
[pipeline research](../../research/rspack-pipeline.md#stage-2-2026-10-13) holds design
comparisons. Long-session heap bounds and real-host confirmation remain
acceptance work, not properties established by those prototype runs.

Real Chrome [Docs host tests](../../ui/src/modules/docs/host/docs-host.test.js)
cover baseline restoration, retained dependencies/CSSOM sheets, repeated starts,
real React selection events across reuse and runtime-listener disposal,
rejecting unmount replacement through the canvas owner and superseded cleanup,
parallel responses across a save, metadata failure, immediate resume, changed
and failed revision checks, missed example updates, supersession and disposal.
[Mount tests](../../ui/src/modules/docs/page/mount-examples.test.js) cover aborts
during asynchronous mounting and image readiness, plus cleanup after failure.
[Content-frame tests](../../ui/legacy/content-frames.test.cjs) and
[session tests](../../ui/legacy/preview-sessions.test.js) cover retained resume,
oldest compatible idle reuse, mixed lenses, the shared budget, expiry, Reload
and disposal. Shipping-code sequences and installed Acme evidence are recorded
separately from these checks in the performance spec.

## Open questions

- **Which page to prepare.** Preparing the most recently selected page's
  chunks, beyond the shared ones, is not specified yet.
