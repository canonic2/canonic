# Workbench server

The Workbench server is what the extension or the standalone command starts
for a space: it
answers the canvas, serves the [page tree](../glossary.md#what-a-space-holds) and
serves pages through their [adapters](../adapters/README.md). This spec owns its
process model (its child processes, their lifetimes, deadlines and runtime
selection), its routes and its protocol with the process that starts it.
The [preview pipeline](../preview-pipeline/README.md) owns compiling and
delivering native pages; the
[extension contract](../integrations/vscode-extension.md#process-boundaries) owns how the
editor starts and stops it.

**Status:** implemented: the Rust server and its host protocol, request
tracing, the Node evaluator, the definition-index evaluator, server-run
Storybook, the page tree, the resident index and the routes below. Not
implemented or unverified: one evaluator for definition indexing, the
performance targets, builds run on Windows and Linux, and portable
highlighting parity; see [implementation gaps](#implementation-gaps).

The [UI/backend independence contract](modules.md#ui-and-backend-independence)
is mandatory. This server owns project data, operations and previewed content;
its domain services do not compose the Workbench interface or initialize its
components. Generic UI asset delivery is host infrastructure and shares this
HTTP process. UI tooling remains independent of project compiler configuration,
caches and lifecycle. The [rendering-flow contract](rendering-flow.md) requires
the UI to load content-only renderers into artboards; the application
`/_workbench/index.html` is an outer host entry, never an artboard destination.
Compiling and rendering project React, docs or Storybook is backend work.

Decided by the owner ([adapters](../adapters/README.md#decisions), 2026-10-16
and 2026-10-17): one Rust runtime. `workbench-server` answers HTTP, holds the
page index resident with one watcher, and runs the compiler. Node runs only as
child processes for project code, and the editor's side becomes messages
between the host and the server.

## The server and its host

`workbench-server serve` is one space's whole HTTP server
([serve.rs](../../server/src/serve.rs)):

```text
workbench-server serve --root <project> --product <workbench folder>
  [--ports 3579,3580,3581,3582,3583,0] [--space-dir <dir>] [--space-key <key>]
  [--untrusted] [--handoff] [--eager-previews] [--trace]
  [--log-limit <bytes>] [--export-retention-ms <ms>]
  [--export-max-archive-bytes <bytes>] [--view-stale-ms <ms>]
```

It owns the space's public loopback port (the first free of the list, where
0 takes any) and answers everything on it: Workbench's browser files, the
project's files, previews from the native compiler, and the space's routes.
It exits when its standard input closes, so it never outlives its host.

The host is the process that starts it: the extension, or the standalone
command `node server/node/server.js <folder>…`. Both start spaces through
[host/](../../server/node/src/modules/host/index.ts), which keeps the
`start(options)` interface: it spawns the server in its own process group,
killed when the server exits, answers its questions with the editor's
callbacks and the capture service, and closes it with the space. A server that
stops unexpectedly is started again on the same port, at most three times a
minute. Without the binary, a space does not start, and says why.

### Messages

One JSON object per line each way
([messages.rs](../../server/src/serve/messages.rs),
[host.rs](../../server/src/host.rs)).

| From | Message | Meaning |
| --- | --- | --- |
| Server | `listening` | The public port, once. |
| Server | `host`, with `question` | A question the host answers with `reply`: `node` (the Node runtime for project code), `capture` (a capture operation; the image or PDF comes back in a temporary file the server reads and removes), `capture-pool` and `capture-pool-close` (an export's renderers), `open`, `handoff`, `spaces`, `space-open`, `stream` (start or stop a native window stream) and `stream-socket`. |
| Server | `log`, `diagnostic` | A record for the editor's log. |
| Server | `shot`, `changed`, `catalog` | A screenshot was saved; the page tree changed on disk; the docs catalog changed. |
| Host | `reply` | The answer to a question, or why it failed. |
| Host | `catalog` | The page tree with everything this machine adds, for `running.config()`. |
| Host | `announce` | Write this server's address for agents again. |
| Host | `index`, `config` | One page-index request; the resolved manifests of a folder. |

Diagnostics are bounded by the server
([diagnostics.rs](../../server/src/diagnostics.rs)): a record keeps a known
level, a short event and at most twenty small details, and a session stops
logging at its byte budget after saying so once.

### Request tracing

`--trace` enables request timings. The extension's `workbench.trace` setting
passes that flag to each space on startup; the standalone Node command accepts
`--trace` too and prints diagnostics to its terminal. Tracing is off by default.
Every response, including space routes, static assets and failures, records
`preview.request.completed` with route, page, scenario, lens, selection ID when
provided, status and `elapsedMs`, and carries `Server-Timing: total;dur=…`.
For streaming responses this measures producing the response headers, not the
lifetime of the stream or network transfer.

Docs work adds `docs-ready`, `markdown`, `highlight`, `warm-wait` and `bundle`
durations when those phases run. Log fields append `Ms` to the header's phase
names. Markdown excludes its nested highlighting time, including first-use
syntax loading. `docs-ready` covers `Space::docs_ready`, including catalog
assembly when needed. `warm-wait` covers the compiler's whole-project `warmed()`
barrier; `bundle` covers the requested lens's subsequent preparation/build.
Live docs return plain code; their document-owned highlight.js worker colours it
locally. Show code requests plain source with `plain=1` and uses that worker too.
These browser tokenization jobs have no server `highlight` phase. Explicit
`/_workbench/docs/highlight?page=…` requests and portable exports still use
server Syntect; traced requests include first-use syntax loading on a blocking
worker in `highlight`. Document and bundle requests have separate records with
the same selection identity. Startup
`docs.warm.started` and `docs.warmed`/`docs.warm.failed` bracket `Space::warm_docs`;
overlapping work alone does not establish time spent queued behind it.

Timers belong to the request task and are absent outside traced requests.
Diagnostics retain the existing size and session budget limits. Browser phase
records use the same bounded logger; see [performance diagnostics](performance.md#diagnostics).

### Processes and connections

Every project-data, operation and project-preview request goes through the
Workbench server, and the server is the only thing that talks to its child
processes (decided by the owner on 2026-10-16 and 2026-10-18).

| Process | Runs | Listens on | Started |
| --- | --- | --- | --- |
| `workbench-server`, one per space | HTTP for the canvas, the page index and its watcher, the compiler (Rspack's Rust crates), serving and pushed updates, the [implementation proxies](implementation-proxy.md) | The space's public loopback port, and one loopback port per proxied implementation origin, because each framed implementation needs an origin of its own | By the host |
| The Node evaluator, one per space | Project code: computed definitions, `workbench.config.ts`, Astro compiling and rendering, Vue's compiler, project JavaScript loaders ([adapters](../adapters/README.md#how-pages-are-served), [compiler jobs](../preview-pipeline/compiler.md#what-runs-in-the-node-evaluator)) | Nothing: stdin and stdout pipes | On first need, in a trusted workspace, on the Node runtime the host gives ([lifetime](#the-node-evaluator)) |
| Definition-index evaluator, current separate path | Computed/Docs listing and non-native glob selection ([evaluate.rs](../../server/src/evaluate.rs)) | Nothing: one JSON request/answer over pipes, then exit | When indexing requires evaluation/selection; killed/reaped on failure or deadline ([lifetime](#the-definition-index-evaluator)) |
| A Storybook the server runs | The project's `storybook dev --preview-only` ([Storybook adapter](../adapters/storybook.md#a-storybook-the-server-runs)) | A local socket only | On the first story request, in a trusted workspace ([lifetime](#a-storybook-the-server-runs)) |

The editor host and the standalone command are Node processes too, outside the
server.

**Evaluator limit.** Each space's server runs at most one Node evaluator, and
every evaluation of project code for that space goes through it: definitions,
`workbench.config.ts`, adapter steps and definition indexing. The limit is per
space, so a host serving several spaces runs one per space. A Storybook the
server runs is not an evaluator: it is a separate preview service running the
project's own code, outside this limit. Definition indexing currently starts
its own short-lived evaluator beside the long-lived one, which is an
[implementation gap](#implementation-gaps) against this limit. A successful
saved literal listing needs no evaluator.

- **Project code stays out of the server and the editor.** Server-side project
  evaluation never runs in the native server or the extension host. Project
  browser code runs in preview documents, including those the capture helper
  opens when taking pictures.
- **No child takes a TCP port.** A child is reached over stdin and stdout
  pipes or a local socket in a private temporary folder (a Unix socket, or a
  named pipe on Windows), so only the server
  can reach it, and a dev server or Storybook the user runs on any port never
  conflicts with it.
- **Children are the server's.** Started on need, restarted on the next need
  after they exit, and stopped with the server. A space that needs no project
  code on the server starts none: the evaluator asks the host for its runtime
  only when a definition, the project configuration or a lens first needs it.
- **The browser reaches a child only through the server:** a page an owned
  child serves is framed through its implementation proxy, whose port the
  child is told it serves, so the addresses it writes into its pages lead
  back to the proxy.
- **Implementations the user runs** (`kind: url`, or Storybook at `url`) are
  read at their own address and framed through their proxy.
- **Native capture stays with the host.** The capture helper is an Electron
  process the host shares between spaces. Window and Simulator video streams
  are the capture part's: the host serves them on a local socket, and the
  server forwards `/_workbench/window/stream` there, upgrades included.

#### The Node evaluator

The evaluator is one long-lived Node process per space, not one per adapter
([node.rs](../../server/src/node.rs),
[evaluator/](../../server/node/src/modules/evaluator/index.ts)), reached over
stdin and stdout with one JSON request per line.

- The server starts it on the first request that needs project JavaScript,
  with the space's trust and configuration, and it loads only what the
  adapters in use need. It asks the host for its Node runtime the first time
  it starts, so a space whose definitions are literals and that has no
  `workbench.config.ts` starts none.
- It waits up to a minute for the runtime, and fails a request that has no
  answer within two minutes, stopping the process.
- When it exits, every waiting request fails and the next request starts a
  new one; there is no proactive restart.
- A change to `workbench.config.ts` or a file it imports stops it, and the
  next need, such as the rebuild that change triggers, starts a new one.
- It is killed with the server. Its error output goes to the server's
  standard error.

#### The definition-index evaluator

The page index starts a short-lived evaluator
([evaluate.rs](../../server/src/evaluate.rs)) when a listing needs evaluation
(computed definitions and all `defineDocs` definitions) or a glob pattern
outside native matching ([page index](../preview-pipeline/page-index.md#parsing-and-evaluation)).

- It answers one JSON request over pipes, then exits.
- Its deadline is the index request's, 120 seconds by default, in the
  one-process `workbench-server index` and the
  [resident index](#the-resident-index-and-watcher) alike. It is killed at the
  deadline or on failure, and reaped.
- The host's bundled Electron runtime is prepared only when evaluation is
  requested; literal-only listing requires neither runtime extraction nor a
  Node subprocess.

#### A Storybook the server runs

A Storybook implementation without `url` runs the project's own Storybook
([Storybook adapter](../adapters/storybook.md#a-storybook-the-server-runs) owns
the command, the socket and its error messages):

- In a trusted workspace only, on the host's Node runtime.
- The first request through the implementation's proxy, or the stories route,
  starts it. It is ready when `/index.json` answers; a start that takes more
  than 120 seconds fails with Storybook's output.
- After it exits, the next request starts it again. It stops with the server.

#### Runtime selection

Both evaluators, the long-lived Node evaluator and the short-lived
definition-index evaluator, run the executable the host's `node` answer names
([host/](../../server/node/src/modules/host/index.ts) `runtime()`): the
[bundled Electron runtime](../review/capture.md#bundled-electron-runtime),
prepared once and run as Node (`ELECTRON_RUN_AS_NODE`), when that runtime is
available, and otherwise the host process's own executable (the standalone
command's Node). They therefore need no separate Node install.
`NODE_OPTIONS` is removed from their environment. A Storybook the server runs
starts on the same executable, also as Node. Previews compile in
`workbench-server` with Rspack's Rust crates and need no Node runtime of their
own. esbuild in the evaluator
bundles definitions and `workbench.config.ts` for evaluation
([compiler](../preview-pipeline/compiler.md#what-runs-in-the-node-evaluator)).
Each platform VSIX bundles the matching pinned esbuild binary at build time;
source checkouts use the package's installed esbuild. Projects install no
compiler, and the installed extension downloads none.

### The page tree

`/_workbench/config` is assembled in Rust
([catalog.rs](../../server/src/catalog.rs)): `workbench.yaml` and
`workbench.local.yaml` read with the manifest rules
([config.rs](../../server/src/config.rs)), previews and docs pages from the
resident index placed where their titles say, links and Workbench lens
mappings checked, the address of a Storybook found running for `url: auto`,
the proxy origin of one the server runs, booted iOS Simulators from
`xcrun simctl` ([simulators.rs](../../server/src/simulators.rs)), and the docs
pages' sources and problems. URL and Storybook implementations are answered
at their proxies, with the address they proxy as `upstream`. A Storybook or
Simulator that is not running leaves the other pages usable and adds one
problem. The answer is assembled fresh on every request.

The host reads configuration only through the binary: the space list and
each space's mark from `workbench-server spaces <folder>…`
([spaces.rs](../../server/src/spaces.rs)), and implementation start commands
from `workbench-server config <folder> [--space <key>]`. A server whose host
lists no other spaces describes its own.

The manifest rules live in [manifest.js](../../contracts/manifest.js), shared by
the browser; [config-parity.test.js](../../server/node/config-parity.test.js)
runs the Rust reader and the Node reader built on them
([config.js](../../server/node/config.js), which only tests run) over a corpus that reaches every rule
and problem and over seeded random configs, and checks that both readers return
the same resolved configuration and problems for each input.

### The resident index and watcher

The server keeps the page index resident: it holds the saved index in memory,
watches the space, and answers each page-tree request without a process per
resolution. A definition added, removed, renamed or retitled is reported as
`changed` once its background index settles; the extension rebuilds the
sidebar and refreshes the canvas for the space being shown, following
[preview placement](../pages/previews.md#sidebar-placement). An edit that only
changes a component reports nothing.

### Previews

Previews, docs lenses and portable builds compile in the server on Rspack's
Rust crates ([compiler.rs](../../server/src/compiler.rs)), with project
JavaScript in the Node evaluator. The catalog configures them for a trusted
workspace whose `previews` is not `false`, with the index's listing; the
server answers the preview routes, pre-warms every listed preview and docs
lens, and reports request timings as diagnostics. Live builds use the
persistent cache in the system temporary folder, keyed by the binary and the
configuration. Project plugin capabilities are Rust descriptors, with no Node
compiler fallback
([plugin decision](../preview-pipeline/plugins.md#decisions)).

## Route inventory

[Access boundaries](access-boundaries.md) owns the current request-origin,
file-serving and body-limit model, including its gaps. Route presence alone
does not establish authentication, containment or uniform mutation guards.

| Routes | Answered by |
| --- | --- |
| `/_workbench/` UI files, `manifest.js`, `yaml.js`, Lucide, `/_workbench/ui/src/`, `/_workbench/contracts/src/` | Host asset delivery: authored JavaScript/CSS and the Docs host HTML served unchanged; TypeScript and tests are not browser assets ([files.rs](../../server/src/serve/files.rs)) |
| `/_workbench/manifest/`, project files, `/_workbench/preview-runtime.js`, `preview-requests.js`, `preview-compat.js` | Files served as they are, with the compatibility script first in each project HTML page; a missing file is a 404 |
| `/_workbench/previews/`, `*.workbench.ts(x)` | The native compiler ([previews.rs](../../server/src/serve/previews.rs)) |
| `/_workbench/config`, `config-file`, `sizes` | The catalog, and config editing that keeps the rest of the file and its comments ([config/edit.rs](../../server/src/config/edit.rs)) |
| `.md` pages, `/_workbench/docs/source`, `revision`, `bundle`, `highlight` | Docs pages ([docs.rs](../../server/src/docs.rs), [space/docs.rs](../../server/src/space/docs.rs)) |
| `/_workbench/stories` | A Storybook's index, read at its address or through its proxy ([storybook.rs](../../server/src/storybook.rs)) |
| `/_workbench/log`, `view`, `spaces`, `spaces/open`, `canvas/space`, `canvas/review`, `open`, `handoff` | Diagnostics, the [agent view](../integrations/agent-context.md) ([agent_view.rs](../../server/src/agent_view.rs)), spaces from the host, opening a configured source through the host, handoff prompts ([handoff.rs](../../server/src/handoff.rs)) sent to the host |
| `/_workbench/capture/*`, `shot` | Requests checked here ([capture.rs](../../server/src/capture.rs)), pictures taken by the host's capture helper, screenshots saved under `.canonic/.handoffs/` |
| `/_workbench/export` | The design export's jobs ([export.rs](../../server/src/export.rs)), with portable builds from [portable.rs](../../server/src/portable.rs) |
| `/_workbench/simulator/*`, `/_workbench/window/stream` | Testrun input and screen size for a declared Simulator; native streams started by the host and forwarded to its socket |

The routes live in [serve/routes/](../../server/src/serve/routes.rs) and answer
with the statuses and messages the canvas expects.

## Verification

- The Workbench suite starts every space through the host and the built
  binary: config, editing, spaces, capture, handoff, the agent view, exports,
  Storybook and Simulator routes (with a stand-in `xcrun`), proxies through a
  URL lens, the restart policy, previews and docs lenses.
- The Rust suite covers the modules above with their own tests, the server's
  routes and a host question through the built binary, the resident index
  and change reports, and exit on closed input.
- The browser smoke scripts in `scripts/` cover retained preview sessions,
  the canvas and its UI, the live mirror, capture, and lenses, including a
  sign-in through an implementation proxy that survives switching lenses and
  sizes.
- Installed-extension evidence for Docs pool compatibility and for runtime
  extraction versus warm config responses is in the research records on
  [Docs integration](../../research/docs-navigation.md#warm-docs-integration-2026-10-23)
  and [runtime preparation](../../research/startup.md#installed-runtime-preparation-2026-10-23).
  It does not establish general installed-webview navigation latency.

## Implementation gaps

- How a host other than the extension and the standalone command starts and
  talks to the server is open: the same stdio messages as the extension, or a
  library interface.
- Definition indexing starts a short-lived evaluator beside the long-lived one.
- First-page, complete-list and edit target acceptance:
  [performance gaps](performance.md#implementation-gaps).
- Builds for Windows and Linux have not been run on those systems.
- Portable code uses Syntect grammars, so token boundaries can differ from
  live highlight.js output. Markdown does not turn bare URLs into links.

## Decisions for the owner

1. **HTTP stack: `axum` on `tokio`, with `hyper` for proxies and loopback
   requests.** Rspack's crates already use tokio, so the compiler shares the
   runtime.
2. **Toolchain:** the package builds with `nightly-2026-04-16`, Rspack's own
   toolchain at the pinned crates.
3. **One server per space.** Whether one process should serve several spaces
   is undecided.
4. **The resident index and watcher live in the server,** so the warm page
   list needs no process per request.
5. **Rust is the direction for suitable host capabilities in the extension.**
   Electron owns screenshot rendering. An additional Rust component needs a
   defined integration contract; a lab prototype does not establish it.

## Open questions

- Whether the browser keeps `manifest.js` as its reader, or moves to the
  server's answer only.
- **Evaluator exit.** Decide whether an unexpected evaluator exit should publish a
  persistent problem before a subsequent request starts its replacement.
