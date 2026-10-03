# Interactive screenshot capture contract

This spec covers the camera download and screenshot-backed handoff from the
current canvas. The [design-system export](export.md) has a different capture
schedule. See [markup.js](../../workbench/markup.js),
[dom-mirror.js](../../workbench/dom-mirror.js),
[capture-sync.js](../../workbench/capture-sync.js),
[electron-capture.js](../../electron-capture.js), and
[capture-helper/main.cjs](../../capture-helper/main.cjs).

## Why a helper exists

The VS Code extension host can run Node code but does not own the compositor
pixels of a webview. The embedded webview also cannot rely on browser display
capture to read its own frame or a foreign-origin iframe. The extension ships
a private Electron application that renders the requested view in a hidden
`BrowserWindow` and reads its pixels with `webContents.capturePage`. This is
web-page capture, not an operating-system screenshot of VS Code.

- Packaging builds a platform and architecture-specific Electron application
  into the VSIX. At runtime, the extension verifies its archive hash, unpacks
  it once into extension storage, and launches it without downloading a
  browser or changing VS Code launch flags. On macOS it has no visible window
  or Dock icon.
- The helper has an isolated profile and a sandboxed renderer without Node
  integration. Only its parent can send newline-delimited JSON commands over
  standard input; page JavaScript has no command bridge. Navigation accepts
  HTTP(S) pages, and local mirror commands require the workbench loopback
  origin. The helper denies page permission requests, downloads, and new
  windows.
- One helper stays warm for interactive work. The extension queues commands,
  collapses superseded speculative preparations, primes the first compositor
  readback, and restarts a dead or stuck helper. If the packaged helper is
  missing or cannot start, the service selects its headless
  Chrome/Chromium/Edge fallback for that server lifetime. A slow or invalid
  page remains a reported page error.

## What is captured

- A local authored page is served on the workbench origin. The browser
  continuously mirrors its active, live document into the helper's inert
  `/_workbench/capture.html` surface. The first update is complete; later
  updates are revisioned patches. Open shadow roots, stylesheets, form and
  scroll state, canvas pixels, readable video frames, animation state, pointer
  position, and annotations are included. The helper restores the pointer before
  reading pixels so CSS hover states appear in the screenshot. Application
  scripts run in the visible preview, not in the capture document.
- Mutations and inputs schedule preparation; a 250 ms heartbeat catches
  changes that do not emit those events. The helper retains unchanged nodes
  and decoded assets. Before a click capture, the workbench flushes the
  current state and pauses speculative updates. A lost mirror base triggers
  a full snapshot retry of that requested state. Preparation resumes after
  capture, including after failure.
- An external URL or Storybook lens is cross-origin. With the cooperative
  preview bridge, its visible iframe sends a live mirror to the same inert
  capture surface. Without the bridge, the helper navigates to the URL in
  its own browser session, restores scroll, waits for fonts and visible
  images, and overlays marks. An iframe login does not automatically sign in
  the helper.
- A Simulator lens shows a separate ScreenCaptureKit stream. Its interactive
  camera/handoff uses the canvas DOM renderer rather than the Electron page
  helper. Simulator Screen Recording permission belongs to the editor's
  macOS identity.

## Outputs and failure behavior

| Action | Request | Result |
| --- | --- | --- |
| Camera | `/_workbench/capture/image` or `/capture/page/image` | JPEG bytes downloaded by the browser; no project screenshot file |
| Handoff | `/_workbench/capture` or `/capture/page`, then `/handoff` | JPEG saved under `.canonic/.handoffs/`; prompt copied by the extension |
| Preparation | `/_workbench/capture/prepare` or `/capture/page/prepare` | Warms the requested view; no image or file |

Capture dimensions match the preview's CSS pixels, including on Retina
displays. The UI requests JPEG at quality 90. API callers may request PNG;
without a format, PNG remains the endpoint default. JPEG is lossy and has no
transparency. Hosted capture reports a native failure instead of silently
running the slower DOM renderer; a standalone `file://` workbench retains that
renderer. A capture may still wait for first-load fonts, images, or a slow page.

The mirror cannot read closed shadow roots, browser-owned UI, origin-tainted
canvas/video pixels, or nested iframe documents. Linked stylesheets load from
their URLs; runtime CSSOM edits to those sheets are not copied. The cooperative
bridge transfers document state, not credentials or application code.

## Verification points

- [capture-helper.test.js](../../capture-helper.test.js),
  [capture-runtime.test.js](../../capture-runtime.test.js), and
  [electron-capture.test.js](../../electron-capture.test.js) cover helper
  packaging, lifecycle, IPC, and fallback.
- [markup-capture.test.js](../../workbench/markup-capture.test.js),
  [capture-sync.test.js](../../workbench/capture-sync.test.js), and
  [capture-page.test.js](../../workbench/capture-page.test.js) cover the
  interactive request, mirror readiness, and inert surface.
