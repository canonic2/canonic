# Implementation proxy

URL and Storybook lenses load through a loopback proxy that the workbench
server runs for each implementation origin. This spec explains why it exists,
what it does, and how it preserves live-page capture without project setup. See [proxies.rs](../../server/src/proxies.rs), the `proxied` config
step in [space.rs](../../server/src/space.rs), and `upstream` in
[lenses.js](../../ui/legacy/lenses.js). The user-facing description is
[Embedding and sign-in](../../docs/lenses.md#embedding-and-sign-in).

**Status:** implemented in `workbench-server`, including proxies for
processes the server runs. Not supported: identity-provider sign-in, absolute
links to the implementation's origin, live capture before the bridge connects
and exact HTTPS origin behavior; see [implementation gaps](#implementation-gaps).

## The problem it solves

A lens shows a page another server owns: a Storybook on `localhost:6006`, an
app's dev server, a staging site. Screenshots and handoffs must show that page
**as the reviewer sees it**: an opened modal or menu, typed text, a scroll
position, a signed-in state.

Workbench captures what the reviewer sees by streaming the live DOM into the
screenshot helper ([capture.md](../review/capture.md)). For pages on the workbench's own
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
  configured address, not the proxy. A process the server runs itself has
  no other address, so it is reached through its proxy too.
- **A stopped implementation answers 502** in the preview frame, naming the
  address that isn't answering.
- **Capture accepts the proxy origins** alongside the configured ones
  (`origins()` in [space.rs](../../server/src/space.rs)).

## A process the server runs

An implementation can be a process the Workbench server runs itself, such
as a [Storybook without a url](../adapters/storybook.md#a-storybook-the-server-runs).
Its proxy is opened like any other, keyed by the implementation, and its
upstream is the process's local socket rather than an address:

- The first request through the proxy starts the process; requests and
  WebSocket upgrades wait until its socket answers, and pass through to it
  as above.
- The process is told the proxy's own port and believes it serves
  `localhost` on it, so `Host` is `localhost:<proxy port>` and the addresses
  it writes into its pages lead back to the proxy.
- When it can't start, the proxy answers 502 with the process's own error
  as the body.
- **People see the proxy address** for it in handoffs and **Open on its
  own**, since it has no other.

## Capture boundary

URL and Storybook documents must receive the bridge without project changes,
retain their implementation-specific absolute paths, and capture live user
state. Every framed implementation uses its own proxy origin. Workbench-served
previews and native streams use their own acquisition paths.

`https` implementations are reached with rustls, accepting any certificate, as
local dev servers use self-signed ones. Without the server binary,
implementation lenses are unavailable. Acceptance on a real implementation
includes an interaction-only state, such as an opened modal, appearing in the
captured image without project changes.

## Verification points

- [proxy.test.js](../../server/node/proxy.test.js), through a URL lens of a
  running server: bridge injection, header and cookie rewriting, redirects,
  request bodies and `Origin`, WebSocket upgrades and a stopped
  implementation.
- [proxies.rs](../../server/src/proxies.rs) tests: an owned process started by
  the first request, told the proxy's port and reached over its socket, and
  one that cannot start answering 502 with its own reason.
- [storybook/owned.rs](../../server/src/storybook/owned.rs) tests:
  a Storybook the server runs, on a socket and told the proxy's port; restart
  after it exits; the fallback without `--preview-only`; a missing install.
- [server.test.js](../../server/node/server.test.js): the config route's proxy addresses and
  `upstream`, including auto-detected Storybooks.
- [lenses.test.js](../../ui/legacy/lenses.test.js): mapping proxy addresses back
  to `upstream`.
- [capture.test.cjs](../../ui/src/app/capture/capture.test.cjs): element names from
  the mirrored copy.
- Manual: [smoke-lenses.cjs](../../scripts/smoke-lenses.cjs) for lens iframes,
  sessions and sign-in through the proxy, and a running Storybook for an interaction-only
  state (such as an opened modal) appearing in a screenshot.

## Implementation gaps

- **Identity-provider sign-in.** An OAuth or SSO callback returns to the
  address registered with the provider, the implementation's own origin, so
  the session isn't set on the proxy and the lens doesn't keep it.
- **Absolute links.** Links and scripts that spell out the implementation's
  full origin leave the proxy. Relative ones stay on it. Response bodies are
  not rewritten.
- **Before the bridge connects**, a lens screenshot falls back to the helper
  loading the URL in its own session ([capture.md](../review/capture.md)).
- **HTTPS implementations** are proxied over plain `http` on loopback, with
  upstream certificate checks off. Cookie attributes and origin identity change.
  Loopback HTTP can still be potentially trustworthy under the
  [Secure Contexts standard](https://www.w3.org/TR/secure-contexts/#is-origin-trustworthy);
  API availability also depends on ancestor contexts, browser permissions and
  embedding policy. Do not infer that every secure-context API is unavailable
  merely because the proxy uses HTTP, or that it behaves exactly like the
  upstream HTTPS origin.
