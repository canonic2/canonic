# Implementation proxy

URL and Storybook lenses load through a loopback proxy that the workbench
server runs for each implementation origin. This spec explains why it exists,
what it does, and why the workbench must not go back to framing
implementations directly. See [proxy.js](../proxy.js), the `proxied` config
step in [server.js](../server.js), and `upstream` in
[lenses.js](../workbench/lenses.js). The user-facing description is
[Embedding and sign-in](../docs/lenses.md#embedding-and-sign-in).

## The problem it solves

A lens shows a page another server owns: a Storybook on `localhost:6006`, an
app's dev server, a staging site. Screenshots and handoffs must show that page
**as the reviewer sees it**: an opened modal or menu, typed text, a scroll
position, a signed-in state.

Workbench captures what the reviewer sees by streaming the live DOM into the
screenshot helper ([capture.md](capture.md)). For pages on the workbench's own
origin, the workbench reads the iframe's document directly. A page on another
origin can't be read: the browser blocks scripts from reaching into a
cross-origin iframe. Only a script running **inside** that page, the preview
bridge (`/_workbench/preview-bridge.js`), can send its DOM out.

Before the proxy, the lens iframe loaded the implementation's own address, so
the bridge ran only if the team added a loader to its Storybook or app. Without
it, a screenshot came from the helper loading the URL afresh in its own
session. That copy showed the initial page, so the screenshot silently left
out whatever the reviewer had done. The reported case (2026-10-03) was a
Customers story with its Create Customer modal open: the canvas showed the
modal, and the screenshot showed the list without it.

Asking teams to change their Storybook or app to fit Workbench is not
acceptable. The proxy lets Workbench add the bridge itself.

## Why a proxy

The script has to get into a page whose HTML another server sends, inside an
iframe Workbench doesn't control. These were considered and rejected:

| Approach | Why it doesn't work |
| --- | --- |
| Teams load the bridge from `.storybook/preview` or their app | Product requirement: projects install nothing for Workbench. |
| VS Code injects the script | Extensions can only `postMessage` to the top document of their webview. There is no API to run script in a nested frame, and no access to the webview's Electron `webContents` or DevTools protocol. |
| The Electron capture helper injects the script | The helper renders a separate, hidden copy for screenshots. The page the reviewer interacts with lives in VS Code's webview, which the helper never touches. |
| The workbench page reads the iframe | Same-origin policy forbids reading a cross-origin iframe. |
| Serve the implementation under a path on the workbench's own origin | Dev servers request absolute paths (`/@vite/client`, `/sb-addons/…`, `/index.json`, `/storybook-server-channel`) that collide with project files and `/_workbench/` routes, and a second implementation makes them ambiguous. |
| Reload the URL in the helper and replay state | Interaction state lives in the page's memory; a fresh load can't reproduce it, and the helper has its own session. |

The workbench server already sees every page it serves and adds its scripts to
them (`withPreviewScripts`). Putting the server in front of implementations
extends that model to pages it doesn't own.

## Requirements

- **Every URL and Storybook lens loads through the proxy.** The config route
  (`/_workbench/config`) gives the browser the proxy address as `base` or
  `url`, and the original as `upstream`. Lens URLs, story URLs, and Storybook
  channel messages all use the proxy origin.
- **One proxy per implementation origin, on its own loopback port**
  (`http://127.0.0.1:<port>`), shared by implementations at the same origin and
  closed with the server. A port, not a path, so the implementation's absolute
  paths keep working unchanged.
- **Pass everything through.** Every method, body, and status, and WebSocket
  upgrades as raw bytes. The implementation sees requests addressed to itself:
  `Host` is its host, and `Origin` and `Referer` are rewritten from the proxy
  origin to its origin.
- **Add the bridge to documents only.** An HTML response to a navigation
  (`Sec-Fetch-Dest` of `document`, `iframe`, or `frame`, or none) gets
  `<script src="<workbench>/_workbench/preview-bridge.js">` first in `<head>`.
  Fetched HTML, scripts, and other assets are untouched. Rewritten documents
  are `no-store`, with no `ETag`, because they name this session's workbench
  port. Requests ask for uncompressed responses so documents can be rewritten.
- **Only the bridge.** The compatibility bundle (`preview-compat.js`) is for
  pages Workbench serves; its form and navigation handling would change how an
  external app behaves.
- **Keep the page in the preview frame.** Drop `X-Frame-Options`,
  `Content-Security-Policy` (and its report-only form), and the cross-origin
  opener, embedder, and resource policies. Remove `Domain`, `Secure`, and
  `SameSite` from cookies so they belong to the proxy origin. Rewrite redirect
  `Location` headers from the implementation's origin to the proxy's.
- **People see the real address.** Handoffs, **Copy reference**, and **Open on
  its own** name the `upstream` address. The proxy port changes every session
  and means nothing outside it.
- **The server talks to implementations directly.** Storybook index reads,
  catalog imports, start checks, and export reference captures use the
  configured address, not the proxy.
- **A stopped implementation answers 502** in the preview frame, naming the
  address that isn't answering.
- **Capture accepts the proxy origins** alongside the configured ones
  (`origins()` in [server.js](../server.js)).

## Do not go back to direct framing

Do not change lenses back to loading an implementation's own address in the
iframe, and do not serve Storybook or another implementation from the
workbench's own origin in place of the proxy. Either one brings back the
original failure: screenshots and handoffs that silently differ from what the
reviewer sees, or a request that teams change their projects. Proposals to
remove the proxy "to simplify" or "because Storybook works without it" must
first meet the requirement above: the live page, as seen, captured with no
project changes. Fix proxy problems in the proxy.

Implementations Workbench doesn't frame are outside this rule: `workbench`
previews are already served by Workbench, and `ios-simulator` and `window`
lenses are native streams.

## Current status

### Verified behavior

Verified 2026-10-03:

- Against a Vite-based Storybook on `localhost:6006`, with no Storybook
  changes: the bridge connected, a modal opened by a real click appeared in the
  screenshot, and a handoff named the element under its annotation from the
  mirrored copy. Storybook's channel WebSocket ran through the proxy. Vite's HMR socket
  connected to port 6006 directly, which works because Vite names its own port.
  No page exceptions.
- With a fixture that sends `X-Frame-Options: DENY`, the page loaded and an
  opened `<dialog>` with its backdrop and input value appeared in the
  screenshot.
- Form sign-in with a `SameSite=Lax` session cookie kept its session across
  reloads and lens switches ([smoke-lenses.cjs](../scripts/smoke-lenses.cjs)).
  One of four runs failed before sign-in completed and was not reproduced.

### Known gaps

- **Identity-provider sign-in.** An OAuth or SSO callback returns to the
  address registered with the provider, the implementation's own origin, so
  the session isn't set on the proxy and the lens doesn't keep it.
- **Absolute links.** Links and scripts that spell out the implementation's
  full origin leave the proxy. Relative ones stay on it. Response bodies are
  not rewritten.
- **Before the bridge connects**, a lens screenshot falls back to the helper
  loading the URL in its own session ([capture.md](capture.md)).
- **HTTPS implementations** are proxied over plain `http` on loopback, with
  certificate checks off. Cookie rewriting makes `Secure` cookies work; pages
  that require a secure context for an API won't get one.

## Verification points

- [proxy.test.js](../proxy.test.js): bridge injection, header and cookie
  rewriting, redirects, request bodies and `Origin`, WebSocket upgrades, and a
  stopped implementation.
- [server.test.js](../server.test.js): the config route's proxy addresses and
  `upstream`, including auto-detected Storybooks.
- [lenses.test.js](../workbench/lenses.test.js): mapping proxy addresses back
  to `upstream`.
- [capture-page.test.js](../workbench/capture-page.test.js): element names from
  the mirrored copy.
- Manual: [smoke-lenses.cjs](../scripts/smoke-lenses.cjs) for lens iframes
  and sessions through the proxy, and a running Storybook for an interaction-only
  state (such as an opened modal) appearing in a screenshot.
