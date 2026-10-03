# Interactive screenshot capture contract

This spec covers the camera download and the screenshot-backed handoff from
the current canvas, and the bundled Electron runtime that renders them. The
[design-system export](export.md) uses the same capture service on its own
schedule. [TypeScript previews](previews.md) run their compiler and server
from the same runtime. See [markup.js](../workbench/markup.js),
[dom-mirror.js](../workbench/dom-mirror.js),
[capture-sync.js](../workbench/capture-sync.js),
[capture-page.js](../workbench/capture-page.js),
[electron-capture.js](../electron-capture.js),
[electron-runtime.js](../electron-runtime.js),
[capture-helper/main.cjs](../capture-helper/main.cjs), and the Chrome
fallback in [capture.js](../capture.js). The user-facing description is
[Markup and handoff](../docs/markup-and-handoff.md#screenshots).

## Why a helper exists

The VS Code extension host can run Node code but does not own the compositor
pixels of a webview. The embedded webview also cannot rely on browser display
capture to read its own frame or a foreign-origin iframe. The extension ships
a private Electron application that renders the requested view in a hidden
`BrowserWindow` and reads its pixels with `webContents.capturePage`. This is
web-page capture, not an operating-system screenshot of VS Code.

## Bundled Electron runtime

- Packaging builds one platform- and architecture-specific Electron
  application from `capture-helper/` with
  [bundle-runtime.cjs](../scripts/bundle-runtime.cjs) and stores it in the
  VSIX as `electron-runtime/runtime.tar.gz`, beside a `runtime.json` that
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
- The runtime is one shared install. The screenshot helper runs it as an
  Electron app. The [TypeScript preview](previews.md) worker runs the same
  executable as Node (`ELECTRON_RUN_AS_NODE`), so previews compile without a
  separate Node installation. The capture helper's environment explicitly
  removes `ELECTRON_RUN_AS_NODE` and `NODE_OPTIONS`.
- The runtime is used only when its `runtime.json` target matches the host,
  the archive is present, the host is not macOS earlier than 13 (Darwin 22),
  and a Linux host has `DISPLAY` or `WAYLAND_DISPLAY`. Otherwise screenshots
  use the [Chrome fallback](#chrome-fallback) from the start, and the preview
  worker runs with the host's own executable.
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

### Chrome fallback

- If the helper cannot be unpacked or started (startup failure or timeout),
  the service retries the request through headless Chrome, Chromium, or Edge
  and keeps that engine for the rest of the server's lifetime. Concurrent
  requests switch once. The same engine is selected directly when the bundled
  runtime is unavailable on the host.
- The browser is `canonic.capture.chromePath` when that setting is set;
  otherwise `CANONIC_CHROME_PATH`, then standard install locations. It runs on a
  temporary profile and is driven over the DevTools pipe. With no browser
  found, captures fail with a message naming `CANONIC_CHROME_PATH`.
- A slow, invalid, or failing page and a command timeout in a running helper
  remain page errors. They never switch a working helper to Chrome.

## What is captured

- **Local authored pages and TypeScript previews.** Both are served on the
  workbench origin; a TypeScript preview renders in the workbench's preview
  host page. The browser continuously mirrors the active, live document into
  the helper's inert `/_workbench/capture.html` surface, inside a sandboxed
  frame that cannot run scripts. The first update is complete; later updates
  are revisioned patches. Open shadow roots, inline and adopted stylesheets,
  form and scroll state, focus, open dialogs and popovers, canvas pixels,
  readable video frames, animation state, pointer position, and annotations
  are included. The helper restores the pointer before reading pixels so CSS
  hover states appear in the screenshot. Application scripts run in the
  visible preview, not in the capture document. The server rejects mirror
  records that carry executable nodes.
- **Synchronization.** Mutations, inputs, frame resizes, and markup changes
  schedule preparation; a 250 ms heartbeat catches changes that do not emit
  those events. The helper retains unchanged nodes and decoded assets. Before
  a click capture, the workbench flushes the current state and pauses
  speculative updates. A lost mirror base triggers a full snapshot retry of
  that requested state. Preparation resumes after capture, including after
  failure.
- **URL and Storybook lenses.** These are cross-origin. With the cooperative
  [preview bridge](../docs/lenses.md#screenshots-through-a-url-lens), the
  visible iframe sends full live snapshots, which the helper renders on the
  same inert capture surface. Without the bridge, the helper navigates to the
  URL in its own browser session, restores scroll, waits for fonts and
  visible images, and overlays marks. An iframe login does not automatically
  sign in the helper, and form input or other interaction in the iframe is
  not reflected.
- **iOS Simulator and window lenses.** These show a native window stream.
  Their camera and handoff use the canvas DOM renderer on the visible frame,
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
screen's file, state or story, and lens, and repeated handoffs of one view get
numbered suffixes.

A handoff prompt names the screen, state, width, saved screenshot, each mark
with its position and the element under it, the lens's implementation and
URL, and any code pointers or TypeScript preview source. The element under a
mark comes from the visible document for local pages, and from the helper's
page for unbridged lenses. The prompt says when elements could not be read.
See [handoff.js](../handoff.js).

## Failure behavior

- Hosted capture reports a native failure instead of silently running the
  slower DOM renderer. A standalone `file://` workbench keeps that renderer.
  A capture may still wait for first-load fonts, images, or a slow page.
- A local page or bridged lens whose document contains an `iframe`,
  `object`, or `embed`, or a canvas or video whose pixels are origin-tainted
  (no CORS), is still captured (decided 2026-10-03). Each such element keeps
  its layout box and renders blank; the rest of the page is captured as
  usual, and the export reference or handoff is produced.
- The mirror cannot read closed shadow roots or browser-owned UI, such as
  native select menus. Linked stylesheets load from their URLs; runtime CSSOM
  edits to those sheets are not copied. The bridge transfers document state,
  not credentials or application code.
- A capture requested before the live mirror has its first snapshot fails
  with "the live preview is still loading".

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

## Current status

### Verified behavior

The requirements above match the code as of 2026-10-03, except for the gaps
below. Not covered by automated tests: the macOS < 13 and display-less Linux
cases, and embedded documents or tainted pixels.

### Implementation gaps

- **Embedded documents and tainted pixels fail the capture.** The mirror
  throws `Live capture cannot yet mirror nested frames or embedded documents`
  for an `iframe`, `object`, or `embed`, and fails on an origin-tainted canvas
  or video ([dom-mirror.js](../workbench/dom-mirror.js)), instead of
  rendering those regions blank. The user guides describe the current failure;
  update them with the fix.
- The Chrome fallback emulates `prefers-reduced-motion: reduce`
  ([capture.js](../capture.js) `Target.prototype.enable`), unlike the helper.
  A page with motion-dependent CSS can look different under the fallback.
- The Chrome fallback does not apply the helper's page hardening (denying
  permissions, downloads, and new windows). Its unbridged lens capture waits
  for the load event and fonts, but not explicitly for visible images.
- A bridged lens handoff carries no element descriptions: the server renders
  the mirror with `capture`, which does not evaluate mark anchors, and the
  browser cannot read the cross-origin frame. Unbridged lens handoffs do name
  elements.

### Open questions

- **Remove the Chrome fallback? (proposed 2026-10-03, pending).** The
  direction is to drop it rather than bring it to parity with the helper,
  unless a specific need justifies it. Removing it would leave these hosts
  without screenshots or export references:
  - Remote workspaces (SSH, Dev Containers, Codespaces). The extension
    declares no `extensionKind`, so it runs on the remote host, which is
    usually Linux without `DISPLAY`, where the bundled runtime is not used.
  - macOS 12 and earlier, where the bundled runtime is not used.
  - A source checkout run with `node server.js` before
    `npm run bundle-runtime`; `electron-runtime/` is not committed.

  Removing it would also remove the `canonic.capture.chromePath` setting and
  `CANONIC_CHROME_PATH`. Repository tooling that drives Chrome directly
  through [capture.js](../capture.js), such as
  [smoke-lenses.cjs](../scripts/smoke-lenses.cjs) and the website's
  screenshot script, is separate from the fallback and needs its own
  decision. If the fallback stays, the parity gaps above apply.
- Should bridged lens handoffs name the elements under marks, for example by
  asking the bridge to describe anchor points?

## Verification points

- [electron-runtime.test.js](../electron-runtime.test.js) covers extraction,
  link and permission preservation, concurrent hosts, pruning, damaged
  archives, and target mismatch. [package-all.test.js](../package-all.test.js)
  covers per-target packaging order.
- [electron-capture.test.js](../electron-capture.test.js) covers the warm
  process, speculative collapse, crash recovery, timeouts, the Chrome
  fallback and when it does not apply, JPEG transport, and the export pool.
- [capture-helper.test.js](../capture-helper.test.js) covers readback
  priming, hover restoration, in-place Storybook switching, and scroll
  restoration for unbridged pages.
- [capture.test.js](../capture.test.js) covers the Chrome pipe, browser
  discovery, overlay scrolling, settling, and hover.
- [markup-capture.test.js](../workbench/markup-capture.test.js),
  [capture-sync.test.js](../workbench/capture-sync.test.js),
  [capture-page.test.js](../workbench/capture-page.test.js), and
  [dom-mirror.test.js](../workbench/dom-mirror.test.js) cover the browser
  request, flush, resync retry, no-DOM-fallback rule, Simulator rendering,
  preparation scheduling, the inert surface, and pointer revisions.
- [server.test.js](../server.test.js) covers payload validation, origin
  checks, mirror validation, JPEG saving and handoff, warm-up, and bridged
  captures. [handoff.test.js](../handoff.test.js) covers the prompt.
- Manual, on a desktop after `npm run bundle-runtime`:
  [smoke-capture.cjs](../scripts/smoke-capture.cjs) (warm reuse, reloads,
  resizing, implementation handoffs, Dock visibility, crash recovery),
  [smoke-mirror.cjs](../scripts/smoke-mirror.cjs) (live mirror fidelity and
  timings), and [benchmark-capture.cjs](../scripts/benchmark-capture.cjs)
  (readback, resize, and encode profiling). Timings from these scripts apply
  to the machine and fixture they ran on, not to VS Code click latency.
  [smoke-lenses.cjs](../scripts/smoke-lenses.cjs) uses an installed Chrome
  to check lens iframes and sessions.
