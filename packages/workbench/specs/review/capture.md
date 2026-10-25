# Interactive screenshot capture contract

This spec covers the camera download and the screenshot-backed handoff from
the current canvas, and the bundled Electron runtime that renders them. The
[design-system export](export.md) uses the same capture service on its own
schedule. The server's Node evaluators also run on this runtime
([processes and connections](../system/server.md#processes-and-connections)).
See [annotations.js](../../ui/legacy/annotations.js),
[dom-mirror.js](../../ui/legacy/dom-mirror.js),
[capture-sync.js](../../ui/legacy/capture-sync.js),
[capture-page.js](../../ui/legacy/capture-page.js),
[electron-capture.js](../../capture/electron-capture.js),
[electron-runtime.js](../../capture/electron-runtime.js),
[capture/app/main.cjs](../../capture/app/main.cjs), and the page scripts
in [capture-scripts.js](../../capture/capture-scripts.js). The user-facing description is
[Annotations and handoff](../../docs/annotations-and-handoff.md#screenshots).

[Annotation editing](../canvas/annotations.md) owns tools, geometry and input
state; this topic owns acquisition, output and completion. [Hosting](../integrations/hosting.md)
owns capability differences and upgrade/shutdown semantics.

## Why a helper exists

The helper also renders export PDFs through its `printExportPage` command,
specified with the [export's output formats](export.md#options-and-output-formats);
the camera retains its image workflow.

The VS Code extension host can run Node code but does not own the compositor
pixels of a webview. The embedded webview also cannot rely on browser display
capture to read its own view or a foreign-origin iframe. The extension ships
a private Electron application that renders the requested view in a hidden
`BrowserWindow` and reads its pixels with `webContents.capturePage`. This is
web-page capture, not an operating-system screenshot of VS Code.

## Bundled Electron runtime

- Packaging builds one platform- and architecture-specific Electron
  application from `capture/app/` with
  [bundle-runtime.cjs](../../scripts/bundle-runtime.cjs) and stores it in the
  VSIX as `capture/runtime/runtime.tar.gz`, beside a `runtime.json` that
  records the target, Electron version, and archive SHA-256. The archive is
  written with normalized metadata so an unchanged helper keeps its hash.
  The tar archive preserves macOS framework links that a VSIX cannot hold.
- At runtime, the extension verifies the archive hash, unpacks it once into
  extension storage under that hash, and launches it without downloading a
  browser or changing VS Code launch flags. Concurrent extension hosts publish
  a complete directory atomically. Once the current runtime is in place,
  runtimes with other hashes and abandoned extraction directories are removed.
  A damaged archive or one built for another platform is rejected before
  anything is published.
- Capture and page-evaluation callers in one host share one in-flight
  preparation for the same cache destination, including archive verification,
  extraction and pruning. Each caller receives its own timing report, marked
  when shared. Completion or failure releases the in-flight entry so a failed
  preparation can retry and a removed cache is checked again. Different storage
  roots remain independent; separate host processes still publish atomically.
  A cached executable needs no archive read. First use of a changed runtime
  can still require extraction before project definitions that need
  evaluation can be listed.
- The runtime is one shared install. The screenshot helper runs it as an
  Electron app; how the server's evaluators run the same executable is in
  [processes and connections](../system/server.md#processes-and-connections).
  The capture helper's environment explicitly removes `ELECTRON_RUN_AS_NODE`
  and `NODE_OPTIONS`.
- The runtime is used only when its `runtime.json` target matches the host,
  the archive is present, the host is not macOS earlier than 13 (Darwin 22),
  and a Linux host has `DISPLAY` or `WAYLAND_DISPLAY`. Otherwise there are no
  screenshots: every capture and export reference fails with the reason (for
  example, "it needs macOS 13 or later"), and the Node evaluator runs with the
  host's own executable.
- On macOS the helper is an accessory app (`LSUIElement`) with no visible
  window or Dock icon, including at startup.

## Helper isolation and lifecycle

- The helper has a temporary, isolated profile and a sandboxed renderer
  with context isolation and no Node integration. Only its parent can send
  newline-delimited JSON commands over standard input; page JavaScript has no
  command bridge. Navigation accepts HTTP(S) pages only, and local mirror
  commands require the workbench's loopback origin. The helper denies page
  permission requests, downloads, and new windows.
- One helper stays warm for interactive work. The extension and the
  standalone server start it with the server and load the inert capture
  surface. The service queues
  commands, collapses superseded speculative preparations to the newest
  view, and primes the first compositor readback during preparation so the
  first click does not pay for it.
- A helper that exits is replaced and given the last requested view again,
  with a growing retry delay (1 s doubling to at most 30 s). A request that
  exits mid-command is retried once on the replacement. A command that runs
  past its timeout (30 s by default) kills the helper; the view that hung is
  not replayed on its own, and the workbench asks again with its own backoff.
- A helper that cannot be unpacked or started fails the request with that
  error. There is no second screenshot engine.

## What is captured

The [multiple-artboard support API](../canvas/multiple-artboards.md) captures every supplied artboard,
including off-camera views and mixed spaces/content. `ui/src/modules/review/capture.js`
composes one labelled JPEG with local annotation coordinates mapped to
regions. Limits are 32 megapixels and 32767px per edge. Loading/error views
have explicit placeholders; a ready-view capture failure stops the operation.
The controller locks ready instances and verifies identity before/after
sequential acquisition. Only captured annotations are cleared, after handoff
success.

The shipping screenshot/handoff controls use the complete canvas snapshot; a
single ordinary view is one region. The acquisition paths below run independently
within each [content-only renderer](../system/rendering-flow.md). They must not acquire a
nested Workbench shell or depend on hidden global screenshot/annotation controls.
Content and annotation factories supply explicit acquisition ports. Canvas
acceptance checks startup requests and child markup alongside full-canvas handoff.

- **Local authored pages and TypeScript previews.** Both are served on the
  workbench origin; a TypeScript preview renders in the workbench's preview
  host page. The browser continuously mirrors the active, live document into
  the helper's inert `/_workbench/capture.html` surface, inside a sandboxed
  iframe that cannot run scripts. The first update is complete; later updates
  are revisioned patches. Open shadow roots, inline and adopted stylesheets,
  form and scroll state, focus, open dialogs and popovers, canvas pixels,
  readable video frames, animation state, pointer position, and annotations
  are included. The helper restores the pointer before reading pixels so CSS
  hover states appear in the screenshot. Application scripts run in the
  visible preview, not in the capture document. The server rejects mirror
  records that carry executable nodes.
- **Synchronization.** Mutations, inputs, preview frame resizes, and
  annotation changes schedule preparation; a 250 ms heartbeat catches changes
  that do not emit those events. The helper retains unchanged nodes and
  decoded assets. Before a click capture, the workbench flushes the current state and
  pauses speculative updates. A lost mirror base triggers a full snapshot
  retry of that requested state. Preparation resumes after capture, including after
  failure.
- **URL and Storybook lenses.** These are cross-origin, loaded through the
  [implementation proxy](../system/implementation-proxy.md), which adds the
  [preview bridge](../../docs/lenses.md#screenshots-through-a-url-lens) to each
  page. The visible iframe sends full live snapshots, which the helper renders
  on the same inert capture surface. Until the bridge has sent its first
  snapshot, the helper navigates to the URL in its own browser session,
  restores scroll, waits for fonts and visible images, and overlays
  annotations. An iframe login does not sign in the helper, and interaction in
  the iframe is not reflected in that fallback.
- **iOS Simulator and window lenses.** These show a native window stream.
  Their camera and handoff use the canvas DOM renderer on the visible artboard,
  not the Electron helper. Screen Recording permission belongs to the
  editor's macOS identity.

## Outputs

| Action | Request | Result |
| --- | --- | --- |
| Camera | `/_workbench/capture/image` or `/_workbench/capture/page/image` | JPEG bytes downloaded by the browser; no project screenshot file |
| Handoff | `/_workbench/capture` or `/_workbench/capture/page`, then `/_workbench/handoff` | JPEG saved under `.canonic/.handoffs/`; prompt copied by the extension |
| Streamed-lens handoff | DOM render, `/_workbench/shot`, then `/_workbench/handoff` | Same saved JPEG and prompt; the camera downloads the DOM render directly |
| Preparation | `/_workbench/capture/prepare` or `/_workbench/capture/page/prepare` | Warms the requested view; no image or file |

The `/page` routes accept only origins of configured implementations; the
local routes accept only the workbench origin. Capture dimensions match the
preview's CSS pixels, including on Retina displays. The UI requests JPEG at
quality 90. API callers may request PNG; without a format, PNG is the
endpoint default. JPEG is lossy and has no transparency. Filenames follow the
page's file, scenario or story, selected iteration, lens and size key, and
repeated handoffs of one view get numbered suffixes.

A handoff prompt names the page, scenario, selected iteration, resolved size and dimensions, saved screenshot, each
annotation with its position and the element under it, the lens's
implementation and URL, and any code pointers or TypeScript preview source.
The element under an annotation comes from the visible document for local
pages, from the mirrored copy on the capture surface for bridged lenses
([capture-page.js](../../ui/legacy/capture-page.js)), and from the helper's page
for the URL fallback. The prompt says when elements could not be read.
See [handoff.rs](../../server/src/handoff.rs).

## Failure behavior

- Hosted capture reports a native failure instead of silently running the
  slower DOM renderer. A standalone `file://` workbench keeps that renderer.
  A capture may still wait for first-load fonts, images, or a slow page.
- A local page or bridged lens whose document contains an `iframe`,
  `object`, or `embed`, or a canvas or video whose pixels are origin-tainted
  (no CORS), must still be captured. Each such element keeps
  its layout box and renders blank; the rest of the page is captured as
  usual, and the export reference or handoff is produced. This is not yet
  implemented: such a capture currently fails (see
  [known capture limits](#known-capture-limits)).
- The mirror cannot read closed shadow roots or browser-owned UI, such as
  native select menus. Linked stylesheets load from their URLs; runtime CSSOM
  edits to those sheets are not copied. The bridge transfers document state,
  not credentials or application code.
- A capture requested before the live mirror has its first snapshot fails
  with "the live preview is still loading".

CSS Custom Highlight ranges travel as root element state, referencing the
snapshot's existing node identities and offsets. The inert receiver recreates
the ranges and their priority/type with the copied stylesheets; it never inserts
syntax spans. Colour-only changes advance the mirror revision, and cleared
highlights are removed on the receiver. Docs capture waits for the worker's
completion or bounded plain fallback before reading that snapshot.

## Decisions

- **Mirror the live document instead of re-running the page.** The helper
  shows exactly what the reviewer sees, including typed input, scroll, and
  hover, without running application code twice or replaying interactions.
- **Fail rather than fall back to DOM rendering when hosted.** DOM rendering
  takes seconds on real pages and renders some fonts and effects differently;
  a visible error is preferable to a slow, divergent image.
- **Keep the OS motion preference in the helper.** Mirrors carry animation
  phases, and forcing reduced motion would select different CSS from the
  visible preview.
- **One screenshot engine.** The bundled helper is the only
  engine. Hosts that can't run it, such as Linux without a display (most
  remote workspaces) and macOS 12 and earlier, have no screenshots rather
  than a second engine that renders differently and needs its own
  hardening.

## Current status

### Verified behavior

The acquisition requirements above run within each artboard; full-canvas
composition and browser support-harness handoffs have recorded evidence. Not covered by automated tests: the macOS < 13 and display-less Linux
cases, and embedded documents or tainted pixels.

### Known capture limits

- **Embedded documents and tainted pixels fail the capture.** The mirror
  throws `Live capture cannot yet mirror nested frames or embedded documents`
  for an `iframe`, `object`, or `embed`, and fails on an origin-tainted canvas
  or video ([dom-mirror.js](../../ui/legacy/dom-mirror.js)). This is a gap
  against the [failure behavior](#failure-behavior) requirement that such
  elements render blank and the rest of the page is captured.

## Verification points

- [electron-runtime.test.js](../../capture/electron-runtime.test.js) covers extraction,
  link and permission preservation, concurrent hosts, pruning, damaged
  archives, and target mismatch. [package-all.test.js](../../scripts/package-all.test.js)
  covers per-target packaging order.
- [electron-capture.test.js](../../capture/electron-capture.test.js) covers the warm
  process, speculative collapse, crash recovery, timeouts, the reason given
  on a host without the runtime, startup failures, JPEG transport, and the
  export pool.
- [capture-helper.test.js](../../capture/capture-helper.test.js) covers readback
  priming, hover restoration, in-place Storybook switching, and scroll
  restoration for unbridged pages.
- [capture-scripts.test.js](../../capture/capture-scripts.test.js) covers overlay
  scrolling, export settling, and the Storybook switch script.
- [annotations-capture.test.js](../../ui/legacy/annotations-capture.test.js),
  [capture-sync.test.js](../../ui/legacy/capture-sync.test.js),
  [capture-page.test.js](../../ui/legacy/capture-page.test.js), and
  [dom-mirror.test.js](../../ui/legacy/dom-mirror.test.js) cover the browser
  request, flush, resync retry, no-DOM-fallback rule, Simulator rendering,
  preparation scheduling, the inert surface, and pointer revisions.
- [server.test.js](../../server/node/server.test.js) covers payload validation, origin
  checks, mirror validation, JPEG saving and handoff, warm-up, bridged
  captures, and proxied implementation addresses in the config.
  [proxy.test.js](../../server/node/proxy.test.js) covers bridge injection, header and cookie
  rewriting, redirects, request bodies, WebSocket upgrades, and a stopped
  implementation. The [handoff tests](../../server/src/handoff/tests.rs) cover the prompt.
- Manual, on a desktop after `pnpm run bundle-runtime`:
  [smoke-capture.cjs](../../scripts/smoke-capture.cjs) (warm reuse, reloads,
  resizing, implementation handoffs, Dock visibility, crash recovery),
  [smoke-mirror.cjs](../../scripts/smoke-mirror.cjs) (live mirror fidelity and
  timings), and [benchmark-capture.cjs](../../scripts/benchmark-capture.cjs)
  (readback, resize, and encode profiling). Timings from these scripts apply
  to the machine and fixture they ran on, not to VS Code click latency.
  [smoke-lenses.cjs](../../scripts/smoke-lenses.cjs) drives an installed Chrome
  through the development driver [chrome.cjs](../../scripts/chrome.cjs) to check
  lens iframes and sessions through the implementation proxy; the extension
  never uses that driver.
