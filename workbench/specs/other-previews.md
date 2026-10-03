# Authored pages and other implementations

This spec covers authored HTML pages and the `url`, `ios-simulator`, and
`window` implementations. They share the selection, address, and frame
behavior in [core.md](core.md). Storybook has its own contract in
[storybook.md](storybook.md). TypeScript previews and the `workbench`
implementation are in [previews.md](previews.md).

## Authored HTML pages

```yaml
sections:
  - name: Pages
    items:
      - label: Sign in
        src: pages/sign-in.html
        states:
          - id: default
            label: Default
          - id: error
            label: Wrong password
```

### Requirements

- `src` is relative to the project root and cannot start with `/` or contain
  `..`, `:`, or `~`. Pages need nothing added for Workbench: no script tag,
  import, or build step.
- A state is declared in YAML, which adds the sidebar row, and in the page,
  which decides what it looks like. Declaring a state in YAML alone does not
  make the page render it. State IDs are kebab-case because they travel in
  addresses and screenshot names. A screen needs at least two states to show
  state rows.
- The first state is the page as authored. The address omits it, and the page
  sees it as `default` whatever its ID. Any other state travels as
  `?state=<id>`.
- Invalid entries are reported as problems rather than silently dropped
  ([core.md](core.md)).

### Compatibility script

- The server injects exactly one script, `/_workbench/preview-compat.js`,
  first in `<head>` (or after `<html>`, or at the top of a fragment) of every
  HTML file it serves from the project. Compiled TypeScript previews receive
  the same injection ([previews.md](previews.md)). The bundle joins
  `keys.js`, `actions.js`, and `states.js`: editor-shortcut forwarding, the
  Actions switch, and state handling. The workbench's own folder never
  receives it.
- A page opened straight from disk gets no script and shows as authored. A
  served page loaded outside the workbench, with no `actions` parameter, has
  actions on.
- `states.js` sets `data-wb-state` on `<html>` immediately: the `state`
  parameter, or `default` when it is absent or not kebab-case. At
  `DOMContentLoaded` it removes elements whose `data-wb-state-only` list
  excludes the state or whose `data-wb-state-not` list includes it, and
  applies the active state's `data-wb-set-<id>` attributes. Elements added
  later are not processed.

### Actions and navigation

- Actions are off by default in the workbench: links do not navigate and forms
  do not submit. Hover, focus, disclosure, pickers, and other in-place changes
  still work. Turning actions on lets the page behave normally. The page's
  `<html>` carries `data-wb-actions="on"` or `"off"`.
- With actions on, a same-origin link or form action to an `.html` page is
  handed to the workbench, which switches to that screen when it is configured
  and reveals it in the sidebar. Other sites, same-page anchors, `mailto:`,
  and downloads keep their browser behavior.
- Browser form validation is turned off (`noValidate`) on the page's forms
  whether actions are on or off, so a `required` field never stops a flow.
- The Actions switch applies only to pages the workbench serves. Through a
  `url`, `storybook`, `ios-simulator`, or `window` lens it is disabled and
  shown as on.

## URL implementations

```yaml
implementations:
  staging:
    kind: url
    base: https://staging.example.com
  dev:
    kind: url
    base: http://localhost:3710
    root: ../acme-web

sections:
  - name: Pages
    items:
      - label: Sign in
        src: pages/sign-in.html
        implementations:
          staging: /sign-in
          dev:
            default: /sign-in
            error: /sign-in?error=1
        code:
          dev: src/routes/sign-in/page.tsx
```

- A URL implementation requires an HTTP(S) `base`, which may include a base
  path. Trailing slashes are removed and the screen's path is appended.
- Each authored screen maps the implementation to one path beginning with `/`,
  or to a map keyed by that screen's declared state IDs. The map needs an
  entry for the first state, under its own ID or `default`. A state without an
  entry uses the default path; a key the screen doesn't declare is a problem.
- With a single path, the lens shows that path for every state and hides the
  state menu. With a state map, the state menu stays and each pick loads its
  path.
- The URL lens opens the external page in an iframe. Apps must permit
  embedding and manage their own session in that context. When embedding
  fails, the failure stays in the frame; Workbench does not switch to another
  way of showing the page. **Open on its own** opens the URL in the browser.
- A URL implementation may use the same optional `start` command and
  `check`/`ready` probes as Storybook ([storybook.md](storybook.md),
  [vscode-extension.md](vscode-extension.md)). This is a VS Code startup
  feature, not a requirement to view a hosted implementation.
- An external page may opt into the cooperative preview bridge,
  `/_workbench/preview-bridge.js`. Without it,
  [screenshot capture](capture.md) visits the URL in the helper's separate
  session, whose authentication and interaction state may differ from the
  visible iframe.

### Implementation roots and code pointers

- `root` on a `url`, `storybook`, `ios-simulator`, or `window` implementation
  says where its code lives, relative to `workbench.yaml` or absolute. A URL
  `root` is rejected.
- A screen's `code` maps implementation names to one path or a list, relative
  to that implementation's `root` or absolute. A screen may list `code` for an
  implementation without mapping a lens to it.
- Code pointers require the implementation's `root`, even for absolute paths.
  Without one, the pointer is listed but cannot be opened, and
  `/_workbench/config` reports that the implementation has no root.
- Paths that don't exist on this machine are kept with `exists: false`, shown
  disabled in **Open the source**, and left out of the handoff.

## iOS Simulator implementations

```yaml
implementations:
  simulator:
    kind: ios-simulator
    device: booted
    catalog: true
```

- `device` defaults to `booted`, which matches every booted device; otherwise
  it is one exact device name or UDID. With `catalog: true`, each matching
  booted, available device becomes an implementation-only screen, labeled with
  the device name, in a section named after the implementation. When no
  matching booted device is found, the catalog reports a problem.
- An authored screen may map a Simulator lens to a device name or UDID instead
  of, or as well as, using a catalog. See the gap below.
- The canvas shows the device through the native window stream described in
  [Native window stream](#native-window-stream). Taps and drags on the canvas
  are sent to the device through WebDriverAgent (WDA), installed by Canonic
  Shield's iOS automation. Without WDA the stream still shows, but input
  doesn't reach the device.
- Simulator lenses have no state menu. `start` is reported as a problem;
  `root` and code pointers work as for other implementations.
- Export reference screenshots are supported for authored design pages,
  TypeScript preview states, and imported Storybook stories. An implementation-only Simulator screen
  receives an export warning instead of a fabricated reference image.
  See the [export contract](export.md) for its capture plan and archive.

## Window implementations

```yaml
implementations:
  emulator:
    kind: window
    app: com.example.emulator

sections:
  - name: Screens
    items:
      - label: Sign in
        src: design/sign-in.html
        implementations:
          emulator: Example Phone
```

- `app` is required: a bundle ID or part of one, in letters, digits, dots,
  and hyphens. A screen maps the implementation to a window title or part of
  one. The helper streams the largest on-screen window whose bundle ID and
  title contain those values, ignoring case. When none matches, the error
  lists up to twelve visible windows.
- The canvas posts the implementation and the screen's `src` to
  `/_workbench/window/stream`. The server answers only when that screen maps
  that `window` implementation, and takes the app and title from the config,
  so a page cannot request any other window.
- Window lenses are view-only: canvas input doesn't reach the app, and there
  is no state menu. `start` and `catalog` are reported as problems. Export
  captures the screen's design page, not the window.

## Native window stream

- Simulator and window lenses use one native helper built from
  [Capture.swift](../window-capture/Capture.swift), which ships as source in
  the extension. The first stream compiles it with `xcrun swiftc`, signs it ad
  hoc, and caches it until the source or `Info.plist` changes. It requires
  macOS 13 or later and Xcode; other platforms get "Native window streaming
  requires macOS."
- The helper captures the window with ScreenCaptureKit and encodes it once
  with VideoToolbox, at up to 900 pixels wide. A Simulator stream asks for the
  Simulator app's window titled with the device name.
- A top-level browser with WebCodecs receives H.264 at about 30 frames per
  second over a WebSocket. An embedded workbench, such as the VS Code webview,
  or a browser without WebCodecs receives JPEG at about 20 frames per second
  over a streamed HTTP response, because VS Code does not promise an H.264
  decoder and its webview forwarding does not reliably keep a WebSocket
  upgrade. Both carry a private per-stream token on the loopback server.
- The server runs one native stream at a time, shared by Simulator and window
  lenses. A request for the same stream (the device's UDID, or the window
  implementation and screen) and codec reuses it; any other request stops it
  and starts a new one.
- macOS attributes Screen Recording to the app that launched the helper: VS
  Code in the extension, or the terminal for a standalone server. A denial
  names that app. There is no fallback to browser screen sharing; when the
  helper can't capture, the canvas says why.

## Current gaps

Found on 2026-10-03 by reading the code and, where noted, by a server probe
with a stubbed Simulator list and stream.

- **Authored Simulator mappings are refused.** The stream, session, and input
  routes accept only a UDID that belongs to a catalogued screen
  (`__ios-simulator/<key>/…`). An authored screen's lens sends its mapped value
  as the UDID ([workbench.js](../workbench/workbench.js) `wbSimulator.show`).
  The probe returned 403 for a device-name mapping with or without
  `catalog: true`, and for a UDID mapping without `catalog: true`; only a
  UDID with `catalog: true` streamed. This violates the requirement that an
  authored screen may name a device by name or UDID. No test covers an
  authored Simulator mapping.
- **Mapped devices are not checked.** Without `catalog: true`, a Simulator
  whose device isn't booted produces no entry in `problems`; the failure
  appears only on the canvas.
- **Invalid authored entries are dropped silently.** A screen whose `src`
  starts with `/` or contains `..`, `:`, or `~`, and a state whose ID isn't
  kebab-case, are left out with no problem ([config.js](../config.js)
  `screen` and `states`), contrary to the reporting requirement in
  [core.md](core.md).
- **Links to unlisted project pages do nothing.** With actions on, the page
  cancels any same-origin `.html` link and hands it to the workbench, which
  ignores destinations that aren't configured screens. Such a link neither
  navigates nor switches screens.

## Open questions

- Should an authored Simulator lens resolve a device name to a booted UDID
  without requiring `catalog: true`, or should the mapping accept only UDIDs?
- The single shared stream means a VS Code canvas (JPEG) and a browser canvas
  (H.264) showing the same device, or two canvases showing different windows,
  replace each other's stream. Is that acceptable, or should one helper serve
  several codecs or sources?
- Should a link to a project page that isn't a configured screen navigate the
  iframe normally, as the user guide implies, or stay blocked?

## Verification points

- [config.test.js](../config.test.js) and
  [manifest.test.js](../workbench/manifest.test.js) cover paths, state maps
  (including a first state named by its own ID), implementation validation,
  window implementations, code pointers, and code for an implementation
  without a root.
- [preview-scripts.test.js](../preview-scripts.test.js) checks that the bundle
  applies actions, states, and `noValidate` together, and that served HTML
  and the compat route use the one injection. [server.test.js](../server.test.js)
  checks placement first in `<head>`, the Simulator catalog, stream
  negotiation only for a configured device or declared window, and export
  warnings for implementation-only screens.
- [simulator.test.js](../workbench/simulator.test.js) checks that a window lens
  streams without starting WDA, and that an embedded workbench uses JPEG.
  [window-stream.test.js](../window-stream.test.js) checks one native stream,
  token checks, HTTP framing, and that the packaged extension ships the
  helper source.
- [lenses.test.js](../workbench/lenses.test.js) checks URL joining.
  [startup.test.js](../startup.test.js) checks start commands.
- Manual: frame rates, Screen Recording prompts, WDA input, and macOS
  version support are checked on a Mac with a booted Simulator and a visible
  app window. No automated test covers them.
