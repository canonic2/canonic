# Authored pages and other implementations

This spec covers authored HTML pages and the `url`, `ios-simulator`, and
`window` implementations. They share the selection, address, and artboard
behavior in [core.md](core.md). Storybook has its own contract in
[storybook.md](storybook.md). TypeScript previews and the `workbench`
implementation are in [previews.md](previews.md).

The keys, their validation rules, and examples are in the
[workbench.yaml reference](../docs/configuration.md); this spec does not
repeat them. It records the behavior those keys produce, the decisions behind
it, and where the code falls short.

## Authored HTML pages

### Requirements

- Pages need nothing added for Workbench: no script tag, import, or build
  step.
- A state is declared in YAML, which adds the state to the page list, and in
  the page, which decides what it looks like. Declaring a state in YAML alone
  does not make the page render it. State IDs are kebab-case because they
  travel in addresses and screenshot names. A page needs at least two states
  to show its states in the page list.
- The first state is the page as authored. The address omits it, and the page
  sees it as `default` whatever its ID. Any other state travels as
  `?state=<id>`.
- Invalid entries are reported as problems rather than silently dropped
  ([core.md](core.md#problem-reporting)).

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
- State handling sets `data-wb-state` on `<html>` before the page renders and
  applies the state markup once, at `DOMContentLoaded`; elements added later
  are not processed. The attributes are described in
  [Pages and states](../docs/pages-and-states.md#states).

### Actions and navigation

- Actions are off by default in the workbench: links do not navigate and forms
  do not submit. Hover, focus, disclosure, pickers, and other in-place changes
  still work. Turning actions on lets the page behave normally. The page's
  `<html>` carries `data-wb-actions="on"` or `"off"`.
- With actions on, a same-origin link or form action to an `.html` page is
  handed to the workbench, which switches to that page when it is configured
  and reveals it in the sidebar. Other sites, `mailto:`, and downloads keep
  their browser behavior.
- A link whose `href` is a fragment of the same page (`#tour`, not a bare `#`)
  scrolls to its target whether actions are on or off. The script scrolls it
  itself, because the warm preview host's `<base>` would otherwise resolve the
  fragment to another document.
- A mounted TypeScript preview claims its links and forms before this
  hand-off; see [previews](previews.md#links-and-navigation).
- Browser form validation is turned off (`noValidate`) on the page's forms
  whether actions are on or off, so a `required` field never stops a flow.
- The Actions switch applies only to pages the workbench serves. Through a
  `url`, `storybook`, `ios-simulator`, or `window` lens it is disabled and
  shown as on.

## URL implementations

Configuration: [Implementations](../docs/configuration.md#implementations) and
[A page's implementations](../docs/configuration.md#a-pages-implementations).

- A page maps the implementation to one path, or to one path per declared
  state. A state without its own path uses the first state's.
- With a single path, the lens shows that path for every state and hides the
  state menu. With a state map, the state menu stays and each pick loads its
  path.
- The URL lens opens the external page in an iframe, through the
  [implementation proxy](implementation-proxy.md). Apps need no embedding
  changes; they manage their own session in that context. When a page fails to
  load, the failure stays in the preview frame; Workbench does not switch to
  another way of showing the page. **Open on its own** opens the
  implementation's own URL in the browser.
- A URL implementation may use the optional `start` command
  ([vscode-extension.md](vscode-extension.md#implementation-startup)). This is
  a VS Code startup feature, not a requirement to view a hosted
  implementation.
- Projects install nothing for capture. The
  [implementation proxy](implementation-proxy.md) adds the preview bridge to
  every page the lens loads, so [screenshot capture](capture.md) shows the
  live page. Lenses must not go back to framing the implementation's own
  address; that spec explains why.

### Implementation roots and code pointers

Configuration: [Code pointers](../docs/configuration.md#code-pointers).

- An implementation's `root` says where its code lives. A page may list
  `code` for an implementation without mapping a lens to it.
- Code pointers require the implementation's `root`, even for absolute paths.
  Without one, the pointer is listed but cannot be opened, and
  `/_workbench/config` reports that the implementation has no root.
- Paths that don't exist on this machine are kept with `exists: false`, shown
  disabled in **Open the source**, and left out of the handoff.

## iOS Simulator implementations

Configuration: [iOS Simulator](../docs/ios-simulator.md#configure-it).

- With `catalog: true`, each matching booted, available device becomes an
  implementation-only page, labeled with the device name, in a collection named
  after the implementation. When no matching booted device is found, the
  catalog reports a problem.
- An authored page may map a Simulator lens to a device name or UDID instead
  of, or as well as, using a catalog. See the gap below.
- The canvas shows the device through the native window stream described in
  [Native window stream](#native-window-stream). Taps and drags on the canvas
  are sent to the device through WebDriverAgent (WDA), installed by Canonic
  Shield's iOS automation. Without WDA the stream still shows, but input
  doesn't reach the device.
- Simulator lenses have no state menu. `start` is reported as a problem;
  `root` and code pointers work as for other implementations.
- Export reference screenshots are supported for authored design pages,
  TypeScript preview states, and imported Storybook stories. An implementation-only Simulator page
  receives an export warning instead of a fabricated reference image.
  See the [export contract](export.md) for its capture plan and archive.

## Window implementations

Configuration: [App windows](../docs/windows.md#configure-it).

- The helper streams the largest on-screen window whose bundle ID contains the
  implementation's `app` and whose title contains the page's mapped value,
  ignoring case. When none matches, the error lists up to twelve visible
  windows.
- The canvas posts the implementation and the page's `src` to
  `/_workbench/window/stream`. The server answers only when that page maps
  that `window` implementation, and takes the app and title from the config,
  so a page cannot request any other window.
- Window lenses are view-only: canvas input doesn't reach the app, and there
  is no state menu. `start` and `catalog` are reported as problems. Export
  captures the page's design, not the window.

## Native window stream

The [multiple-artboard support](multiple-artboards.md) owns native consumers
per instance. `src/native-streams/pool.ts` creates independent producers by
source and codec. Closing a consumer preserves subscribed peers; unused
producers expire. Simulator input stays device-specific; windows are view-only.
Repeated artboards on one device/window share that external surface.

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
- Each open canvas keeps its own stream (decided 2026-10-03). A VS Code
  canvas and a browser canvas showing the same device, or two canvases showing
  different devices or windows, stream at the same time; opening one must not
  stop or replace another's stream. Requests for the same source and codec may
  share one capture.
- macOS attributes Screen Recording to the app that launched the helper: VS
  Code in the extension, or the terminal for a standalone server. A denial
  names that app. There is no fallback to browser screen sharing; when the
  helper can't capture, the canvas says why.

## Current gaps

Found on 2026-10-03 by reading the code and, where noted, by a server probe
with a stubbed Simulator list and stream.

- **Mapped devices are not checked.** Without `catalog: true`, a Simulator
  whose device isn't booted produces no entry in `problems`; the failure
  appears only on the canvas, contrary to the shared problems list in
  [core.md](core.md#problem-reporting).
- **Invalid authored entries are dropped silently.** A page whose `src`
  starts with `/` or contains `..`, `:`, or `~`, and a state whose ID isn't
  kebab-case, are left out with no problem ([config.js](../config.js)
  `page` and `states`), contrary to the reporting requirement in
  [core.md](core.md).
- **Links to unlisted project pages do nothing.** With actions on, the page
  cancels any same-origin `.html` link and hands it to the workbench, which
  ignores destinations that aren't configured pages. Such a link neither
  navigates nor switches pages.

## Open questions

- Should a link to a project page that isn't a configured page navigate the
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
  warnings for implementation-only pages.
- [simulator.test.js](../workbench/simulator.test.js) checks that a window lens
  streams without starting WDA, and that an embedded workbench uses JPEG.
  [window-stream.test.js](../window-stream.test.js) checks each native producer's
  stream protocol, while `src/native-streams/pool.test.ts` covers producer sharing,
  token checks, HTTP framing, and that the packaged extension ships the
  helper source.
- [lenses.test.js](../workbench/lenses.test.js) checks URL joining.
  [startup.test.js](../startup.test.js) checks start commands.
- Manual: frame rates, Screen Recording prompts, WDA input, and macOS
  version support are checked on a Mac with a booted Simulator and a visible
  app window. No automated test covers them.
