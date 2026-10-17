# Workbench server

The Workbench server is what the extension or Studio starts for a space: it
answers the canvas, serves the [page tree](glossary.md#what-a-space-holds) and
serves pages through their [adapters](adapters/README.md). This spec owns its
process model and its move into the Rust binary `workbench-server`. The
[preview pipeline](preview-pipeline/README.md) owns compiling and delivering
native pages; the [extension contract](vscode-extension.md#process-boundaries)
owns how the editor starts and stops it.

Status: agreed by the owner on 2026-10-17, with the
[decisions](#decisions-for-the-owner) as recommended. Phase 1 is
implemented; its measurement against the current server has not been made.
Phases 2 to 5 are not implemented.

## Current state

- `server/node/server.js` is the HTTP server. The extension `require`s it and
  runs it inside the VS Code extension host, one server per space. The
  standalone `node server/node/server.js` does the same in its own process.
- It talks to the editor through in-process callbacks: opening a file, the
  screenshot capture service, logging, announcing its address and telling the
  sidebar the page tree changed.
- `workbench-server serve`, the front door, owns the space's public loopback
  port (the first free of 3579–3583, then any free port) and forwards every
  request to `server.js` on a private one; it keeps the page index resident
  and watches the space (phase 1 below). Without the binary, `server.js`
  listens publicly itself and runs the index as a process per request.
- Previews compile in the Node preview worker, a child process the server
  forwards `/_workbench/previews/` to.

## Target

Decided by the owner ([adapters](adapters/README.md#decisions)): one Rust
runtime. `workbench-server` answers HTTP, holds the page index resident with
one watcher, and runs the compiler. Node runs only as child processes for
project code. The editor-side callbacks become messages between the extension
and the server.

## Migration

The Rust binary becomes the front door first, then takes routes over one at a
time, so Workbench keeps working at every step.

### Phase 1: the front door

- `workbench-server serve` listens on the space's public loopback port, with
  the current port search, and forwards every request to `server.js` on a
  private loopback port: methods, headers, bodies, streamed and server-sent
  responses, `HEAD`, aborted requests, and WebSocket upgrades (the window
  stream). An unreachable upstream answers 502 with a message, never a hang.
- `server.js` stays where it runs today, with its callbacks. It listens on an
  ephemeral port, starts `workbench-server serve` with that port, and
  announces the address the front door reports. The front door exits when its
  parent's stdin closes.
- `server.js` takes the public origin as an option. Its same-origin check
  (which today compares `Origin` with its own port) and every URL it builds
  for the browser or the capture helper use the public origin.
- The front door keeps the page index resident: one `workbench-server` process
  holds the saved index in memory, watches definition files, and answers
  `server.js` over its private connection instead of a process per
  resolution. A definition added, removed, renamed or retitled tells the
  canvas and sidebar to refresh, which closes the
  [definition watching gap](previews.md#sidebar-placement).

Acceptance: the Workbench suite passes with every HTTP test going through the
front door; Rust integration tests cover each forwarding case above; and the
lab's first-page, page-list and switching sequences stay within 10% of the
current server's p50.

Implemented as above. If the front door stops unexpectedly, `server.js`
starts it again on the same public port, at most three times a minute; after
that, or when a restart fails, `server.js` listens on the public port itself.
The front door runs in its own process group, killed when it exits, so its
evaluator cannot outlive it. The extension rebuilds the sidebar
and refreshes the canvas on a page-tree change for the space being shown.
Verified: the Workbench suite passes through the front door, including a
definition added on disk being reported and listed, and the fallback without
the binary; the Rust tests cover forwarding, streaming, `HEAD`, 502,
upgrades, exit on closed input, the resident index and change reports.
On this repository's own spaces through the standalone server and headless
Chrome (2026-10-17): every page tried in the Acme demo and Website spaces
renders without errors, a definition added or removed shows in the page tree
within about 70 ms and is announced within about 220 ms, and a killed front
door answers again on the same port within about 70 ms. Not yet verified:
the lab measurement and the extension's rebuild in a running VS Code.

### Phase 2: the page tree

`/_workbench/config` is answered in Rust: `workbench.yaml` and
`workbench.local.yaml`, the manifest rules, previews and docs from the
resident index, the iOS Simulator catalog, and problems. The manifest rules
live in [manifest.js](../contracts/manifest.js), shared by the browser and the
server; the Rust reader must give the same answer, checked by a parity
harness that runs both readers over the same fixtures, as the page index's
parity tests do. Storybook `url: auto` detection moves with it.

### Phase 3: files and assets

The canvas's own files under `/_workbench/`, its TypeScript modules (stripped
of types in Rust), project files with the compatibility script injected,
`.md` pages, and the config files. The routes that need the editor or the
capture helper (open, capture, handoff, export, simulator input and streams)
stay forwarded until the callbacks become messages.

### Phase 4: previews

`/_workbench/previews/` moves from the Node worker to the Rust compiler host,
after the next lab settles the [adapters](adapters/README.md#open-questions)
that need it. This is when the binary moves to Rspack's nightly toolchain.

### Phase 5: the server owns the process

The extension starts `workbench-server` as the space's process. Opening files,
capture, logging and page-tree notifications become messages over its stdio;
`server.js` is gone, and Node runs only as the adapters' and the page index's
child processes.

## Route inventory

| Routes | Today | Moves in |
| --- | --- | --- |
| `/_workbench/config` | Config resolution with previews, docs, Storybook detection, Simulator catalog | Phase 2 |
| `/_workbench/` files, `manifest.js`, `yaml.js`, Lucide, `/_workbench/manifest/` | Static files from the extension and the space | Phase 3 |
| `/_workbench/ui/src/`, `/_workbench/contracts/src/` | TypeScript stripped on demand | Phase 3 |
| Project files, `.md` pages, docs source, revision and bundle | Files with injection; docs through the compile worker | Phase 3 (files), phase 4 (docs bundles) |
| `/_workbench/preview-runtime.js`, `preview-requests.js`, `preview-compat.js` | Preview runtime files | Phase 3 |
| `/_workbench/previews/`, `*.workbench.ts(x)` | Forwarded to the preview worker | Phase 4 |
| `/_workbench/sizes`, `config-file`, `spaces`, `spaces/open`, `canvas/space`, `canvas/review`, `log`, agent view | Config editing, spaces, logging, agent context | Phase 3, or 5 where they reach the editor |
| `/_workbench/open`, `shot`, `capture/*`, `handoff`, `export`, `stories` | Editor, capture helper and export | Phase 5 |
| `/_workbench/simulator/*`, `/_workbench/window/stream` (WebSocket) | Native streams and Sandbox input | Phase 5 |

## Decisions for the owner

1. **HTTP stack: `axum` on `tokio`, with `hyper` for forwarding.** Rspack's
   crates already use tokio, so the compiler joins the same runtime.
2. **Toolchain: stable Rust 1.99 until phase 4,** when the compiler brings
   Rspack's nightly.
3. **One front door per space,** as servers are today. One process for every
   space is a Studio question for later.
4. **Phase 1 includes the resident index and watcher,** not only forwarding,
   so the first step already makes the warm page list faster and closes the
   watching gap.

## Open questions

- How Studio starts and talks to the server: the same stdio messages as the
  extension, or a library interface.
- Whether the parity harness of phase 2 keeps `manifest.js` as the reference
  for the browser, or the browser moves to the server's answer only.
