# Storybook adapter

Part of the [adapters](README.md). User guide: [Storybook](../../docs/storybook.md).
The [Storybook contract](../pages/storybook.md) owns configuration, story identity,
story switching, screenshots and export; this spec owns how the Workbench
server gets Storybook to render a story.

## Source

A page maps to one exact Storybook title, and its stories are the page's
scenarios. Storybook renders them with the project's own Storybook, so its
addons, decorators, loaders, `preview.ts` and builder configuration apply.

## How a story is served

Workbench uses two parts of Storybook: `/index.json` for the titles and
stories, and `iframe.html?id=<story>&viewMode=story` for rendering one. It
never loads Storybook's manager interface. The canvas frames stories
through the Workbench server's
[implementation proxy](../system/implementation-proxy.md), never from Storybook's
own address.

## A Storybook the server runs

A Storybook implementation without `url` is the Workbench server's
([storybook.rs](../../server/src/storybook.rs)):

- **It runs the project's own Storybook.** `root`, the space root unless
  set, is the folder with `.storybook/`; the `storybook` package and a
  framework are installed there. The server starts the installed
  `storybook dev --preview-only --exact-port -p <port> --ci --no-open --quiet
  --no-version-updates --disable-telemetry` from that folder. A version that
  rejects `--preview-only` is started again without it.
- **On first need.** When it starts, when it counts as ready, its start
  deadline and when it stops are the server's
  [process lifetimes](../system/server.md#a-storybook-the-server-runs).
- **No TCP port.** A preload
  ([socket-listen.cjs](../../server/node/src/modules/storybook/socket-listen.cjs)),
  loaded with `--require`, moves every TCP listen in that process to a
  local socket in a private temporary folder: a Unix socket, or a named pipe
  on Windows. Only the Workbench server reaches it, through the
  implementation's [proxy](../system/implementation-proxy.md), so a Storybook the
  user runs on 6006 or any other port never conflicts with it.
- **Told the proxy's port.** `<port>` is the proxy's own port. Storybook's
  pages carry addresses built from it, such as Vite's live-update client
  (`hmrPort`), so they lead back to the proxy; hot updates and WebSockets
  pass through it to the socket.
- **Trusted workspaces only.** Storybook runs project code. In an untrusted
  workspace the config reports `implementations › <key>: Workbench runs
  Storybook in a trusted workspace only, since Storybook runs project code.`
- **Failures name the cause.** The lens shows Storybook's own error, and
  the stories route answers `Workbench couldn’t run Storybook in <root>:
  <output>` or `Storybook isn’t installed in <root>. Install storybook and a
  framework there, or set url to a Storybook you run.`
- **A Storybook the user runs** is read at its `url`; its configuration,
  including which keys are problems, is the
  [Storybook contract](../pages/storybook.md)'s.

## Decisions

Decided by the owner on 2026-10-17:

- **Pages are declared in `workbench.yaml`.** Each Storybook page is a page
  of the space mapped to one exact title. Storybook has no catalog import
  that turns every title into a page, so every page of a space comes from
  its own configuration or its definitions.
- **Storybook for React Native is not supported.** React Native components
  render through the [React Native Web adapter](react-native-web.md): less
  exact than a device, and right for designing in the browser beside the
  code.
- **Stories are not rendered with Workbench's own adapters.** Storybook's
  portable stories API would need no Storybook server, but it applies only
  preview annotations, not builder configuration or framework presets.

Decided by the owner on 2026-10-18:

- **The server runs Storybook on a local socket, never a TCP port,** and
  reaches it only through itself. The same holds for any child process an
  adapter needs, such as the Node evaluator for Astro renders.

Verified on 2026-10-18 with Storybook 10.6.2, `@storybook/html-vite` and
Vite 8: ready in about 1.7 to 2 seconds with `--preview-only`; the index,
`iframe.html`, Vite's modules and its live-update WebSocket all answered
through the socket; editing a stories file updated the framed story in
place; nothing listened on 6006. Vite writes the port it is given into its
live-update client, which is why Storybook is told the proxy's port.

## Status

Implemented in the Rust Workbench server, for both a Storybook it runs and
one the user runs ([server](../system/server.md)). The Storybook it runs is started on
the host's Node runtime.
