# Docs host

Part of the [preview pipeline](README.md). This spec covers how Docs pages are
served, highlighted, compiled and hosted on the canvas: the server's Markdown
page, the page before its examples, the dedicated warm Docs host and its pool,
return freshness, document isolation, the reusable page lifecycle and their
implementation constraints. [Docs](../pages/docs-pages.md) owns the authoring
and reading contract. The [performance contract](../system/performance.md#measured-interactions)
owns the improvement goal, and [retained sessions](warm-runtime.md#retained-sessions)
own the retention budget Docs share with other content.

**Status:** Implemented: server Markdown pages and asynchronous highlighting;
the warm Docs pool (the lab's `pool-rp` design), immediate retained resume, the
background return check, document isolation and the reusable page lifecycle in
`ui/src/modules/docs/`, with host-kind routing and idle selection in
`content-frames.js`/`preview-sessions.js`. This topic records no
implementation gap; the Docs example hot-update limit is in
[hot updates](hot-updates.md#implementation-gaps) and Show code's extraction limits are in
[design constraints](#design-constraints).

## Requirements

### Serving and compilation

- **Manifest.** `manifest.js` accepts `.md` sources, `lens`, and the `docs`
  kind, and rejects a `src` that contains `:`, `+`, `!` or `~`, for the
  browser and the server alike; every config reader also rejects a `src` that
  starts with `/` or contains `..`.
- **Serving.** The server serves a docs page at its Markdown path and renders
  the Markdown to HTML with example placeholders. Markdown parsing and syntax
  highlighting use pinned dependencies shipped in the extension, like Lucide;
  no runtime dependency downloads or external highlighting service. Live HTML
  contains escaped, readable code, coloured in the browser
  ([syntax highlighting](#syntax-highlighting)).
- **The page before its examples.** The page answers with its Markdown without
  waiting for a build: its panels are `pending` until the page script learns
  the lens's bundle, which then mounts them or names why not. The examples are
  usually built already by
  [pre-warming](warm-runtime.md#pre-warming-on-project-start).
- **Compilation.** Each docs page lens is an entry of the
  [preview pipeline](compiler.md#entries), built from that
  lens's example source with the lens's adapter, styles, environment, and the
  project's `workbench.config.ts`. Lenses share the previews' compilers, cache
  and trust rules.
- **Source text.** The server returns an example's source text for **Show
  code**: the whole file for directory examples, the export's declaration for
  named exports.
- **Which lens shows.** [lenses.js](../../contracts/src/docs/lenses.js)
  decides a page's lenses, the lens it shows, its default lens, the lens a
  docs link opens, the lens the address names, and the canvas mode. It is
  pure; the canvas loads it through
  [app/workbench/docs.js](../../ui/src/app/workbench/docs.js) and waits for it
  before applying the config, and Node tests check it. Off a server
  (`file://`) the module can't load and the canvas shows no docs lenses.
- **Docs entries.** The server keeps one entry per Markdown file, naming the
  page it belongs to ([docs.rs](../../server/src/docs.rs)); the first page
  to claim a file has it. Docs are served at the Markdown's path in the lens
  `?lens=` names, and the routes take the Markdown as `page`. Handoffs, the
  source menu, and export references use the owning page.

### Syntax highlighting

- One lazy, document-owned module worker uses pinned local highlight.js
  (version and hashes in `ui/src/vendor/highlight.js/manifest.json`) with explicitly registered languages for fences and Show code. It
  starts after document load and two animation frames, outside page
  readiness. Tokenization runs off the browser's main thread; CSS Custom
  Highlight ranges colour the original text nodes without syntax spans.
- CSS, fonts, images and examples retain their readiness waits; asynchronous
  syntax colours alone may finish after the page is visible
  ([readiness](readiness.md#what-readiness-waits-for)). Token colours cannot
  change code font metrics.
- Requests time out to readable plain code; edits/disposal cancel pending work
  and stale results cannot overwrite new text.
- Captures wait for colouring to settle, or for the bounded plain-text failure
  fallback.
- Portable docs are highlighted at export time and never need a live endpoint.

### Warm Docs pool

This section owns document reuse, isolation, freshness and resource bounds
between Docs destinations.

The canvas uses a dedicated Docs host, separate from the preview host. The
host document and its entry are under `ui/src/app/docs/`, served as
`/_workbench/ui/src/app/docs/docs-host.html`, and the Docs capability's reusable
page lifecycle is the `docs` view under `ui/src/views/docs/`. The host is a content-only
document; neither initializes the Workbench shell. The
host reports readiness, visible examples for handoffs, and scroll for
annotations.

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
  iframe to the new destination. The old page stops being a retained return.
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

This is the Docs return check. `resume()` makes an already-ready retained page
available for immediate promotion, then checks its revision once in the
background to catch a missed push. This is a return-time check, not periodic
polling. An unchanged revision does no work on the page. A changed revision
refreshes the same destination, keeping scroll and the selected scenario/lens.
Markdown-only changes follow the in-place
[update contract](hot-updates.md#docs-pages), preserving mounted examples;
example changes follow the adapter's hot-update or remount rules. Push updates
remain the normal edit path; the return check catches an update missed while
idle or disconnected.

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

The reusable page's `start(options)` and `stop()` contract is the
[reusable page lifecycle](#reusable-page-lifecycle). The host awaits cleanup before
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

### Reusable page lifecycle

- `docs-page.js` exposes `start(options)` and `stop()`. Install document-level
  interaction listeners once; they dispatch only to the current page. Importing
  the module does not mount a page. A second start stops the earlier lifecycle
  and mounts fresh, without duplicate tool/link handlers or revision listeners.
- Each start owns its options, action log, source cache, mounted examples,
  highlighting, readiness and revision subscription. `stop()` disposes mounts,
  cancels pending work and releases that state; it is safe after partial startup
  and safe to repeat. Async work from a stopped page cannot report actions or
  readiness, update code panels or affect its successor. Host-level listeners
  and retained runtime assets survive until the host is disposed.
- A cleanup rejection is sticky for that page module's document: later
  start/stop calls continue to reject instead of mounting into uncertain state.
  The warm host must replace that document and let its owner retry the current
  destination under [the isolation contract](#docs-document-isolation).
  Removing a closing/disposed frame does not retry or reopen it.
- Served pages opened on their own explicitly start once, with their existing
  page options. Portable exports do the same with their static metadata and
  pre-highlighted code, without requiring the warm host or live revision,
  metadata or highlighting endpoints. Preserve relative asset/module paths and
  standalone link behavior.
- Reassignment mounts a fresh page; resuming the exact retained destination
  does not call start again. Markdown updates preserve mounted examples where
  the [existing edit contract](hot-updates.md#docs-pages)
  permits, including open code panels with fresh source text. Page tools,
  scenarios, actions, docs links and example anchors retain the same behavior in
  warm, standalone and portable delivery.

### Resource bounds

- Docs use the [retained-session budget](warm-runtime.md#retained-sessions)
  shared with previews. The Docs pool creates no extra spare host outside
  those sessions; protected replacements follow the existing loading
  lifecycle.
- The accepted Docs cost is about 13 MB more JS heap with four documents
  (one active and three idle), based on the heavy-fixture long walk
  (decided 2026-10-23). It does not authorize more retained frames or
  unbounded growth. See
  [measured conditions](../../research/docs-navigation.md#warm-docs-pool-decision-2026-10-23).

## Design constraints

- Prose and examples share the light DOM for annotations, anchors and capture.
  Prose rules exclude `.wb-docs-example` descendants. Highly specific project
  rules can still restyle prose; keep that limitation explicit.
- Named-export source text is extracted by column-zero declaration boundaries,
  including attached comments/decorators, without introducing a second parser.
  The compiler lists export names. Re-exports and multiline templates require
  dedicated source-location handling; see
  [export_source.rs](../../server/src/docs/export_source.rs).
- Docs render as content through the [dedicated renderer contract](../system/rendering-flow.md).
  A docs page or example does not initialize Workbench navigation, toolbar or canvas.
  Project docs/example compilation remains a backend content service; UI asset
  delivery serves/copies browser-native JavaScript/CSS unchanged, with no UI
  compilation or bundling.
- Previews and docs examples share compiler profiles, aliases, rules, plugins
  and environments. React Native Web examples use a column flex stage without
  imposing viewport height on panels.

## Verification

Real Chrome [Docs host tests](../../ui/src/app/docs/docs-host.test.js)
cover baseline restoration, retained dependencies/CSSOM sheets, repeated starts,
real React selection events across reuse and runtime-listener disposal,
rejecting unmount replacement through the canvas owner and superseded cleanup,
parallel responses across a save, metadata failure, immediate resume, changed
and failed revision checks, missed example updates, supersession and disposal.
[Mount tests](../../ui/src/views/docs/mount-examples.test.js) cover aborts
during asynchronous mounting and image readiness, plus cleanup after failure.
[Content-frame tests](../../ui/legacy/content-frames.test.cjs) and
[session tests](../../ui/legacy/preview-sessions.test.js) cover retained resume,
oldest compatible idle reuse, mixed lenses, the shared budget, expiry, Reload
and disposal. [Highlighting tests](../../ui/src/views/docs/highlighting.test.js)
cover the pinned highlight.js assets and worker colouring after load,
preserving every character and settling captures. The pool's
[decision and measured limits](../../research/docs-navigation.md#warm-docs-pool-decision-2026-10-23)
and its [shipping-code and installed Acme evidence](../../research/docs-navigation.md#warm-docs-integration-2026-10-23)
are recorded separately from these checks.
