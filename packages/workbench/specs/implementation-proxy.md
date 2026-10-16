# Implementation proxy

URL and Storybook lenses load through a loopback proxy that the workbench
server runs for each implementation origin. This spec explains why it exists,
what it does, and how it preserves live-page capture without project setup. See [proxy.js](../server/node/proxy.js), the `proxied` config
step in [server.js](../server/node/server.js), and `upstream` in
[lenses.js](../ui/legacy/lenses.js). The user-facing description is
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

Workbench injects the bridge through a loopback proxy so teams need no
Storybook or application changes. Capturing a fresh browser session cannot
stand in for the live page's in-memory interaction state.

## Why a proxy

The proxy supplies document injection and a dedicated origin for each
implementation, preserving root-relative dev-server routes and WebSocket
traffic. Each implementation's origin remains distinct from Workbench's own
project and control routes. The injected script reports the live document;
interaction state is not reconstructed in a separate application session.

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
  (`origins()` in [server.js](../server/node/server.js)).

## Capture boundary

URL and Storybook documents must receive the bridge without project changes,
retain their implementation-specific absolute paths, and capture live user
state. Every framed implementation uses its own proxy origin. Workbench-served
previews and native streams use their own acquisition paths.

## Current status

### Verified behavior

The proxy and capture tests below cover transport, injection, origins and
mirrored element names. The lens smoke scenario checks sessions and sign-in.
Real implementation acceptance includes an interaction-only state, such as an
opened modal, appearing in the captured image without project changes.

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

- [proxy.test.js](../server/node/proxy.test.js): bridge injection, header and cookie
  rewriting, redirects, request bodies and `Origin`, WebSocket upgrades, and a
  stopped implementation.
- [server.test.js](../server/node/server.test.js): the config route's proxy addresses and
  `upstream`, including auto-detected Storybooks.
- [lenses.test.js](../ui/legacy/lenses.test.js): mapping proxy addresses back
  to `upstream`.
- [capture-page.test.js](../ui/legacy/capture-page.test.js): element names from
  the mirrored copy.
- Manual: [smoke-lenses.cjs](../scripts/smoke-lenses.cjs) for lens iframes
  and sessions through the proxy, and a running Storybook for an interaction-only
  state (such as an opened modal) appearing in a screenshot.
