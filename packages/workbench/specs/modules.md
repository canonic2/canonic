# Workbench structure and module ownership

## Product parts

`packages/workbench/` owns the Workbench product. Applications and libraries are
peers; the VS Code extension is one delivery host. The product root coordinates
checks, builds, version consistency and releases, and holds product-wide specs,
user docs, design fixtures and labs.

| Part | Responsibility and boundary |
| --- | --- |
| `extension/` | VS Code activation, commands, webviews, trust input and lifecycle of editor-launched sessions; extension manifest and icons |
| `server/` | Rust page index in `src/`; shipping Node HTTP server, project services and compiler worker in `node/` |
| `capture/` | Electron screenshot renderer, runtime preparation, platform window capture and native stream pooling |
| `ui/` | Browser canvas, components, themes, preview runtimes/adapters and portable viewer |
| `sdk/` | Published definition helpers and authoring types, independently consumable by projects |
| `contracts/` | Shared manifest/YAML interpretation, wire types and pure rules used across parts |
| `scripts/` | Product build coordination, temporary distribution assembly and release tooling |

The server owns project state, definition discovery, compilation, output serving
and subordinate compiler processes. The extension owns editor integration and
the sessions it starts. Capture owns rendering resources. Browser controllers
request operations through host contracts; elements render and emit events.
Clients must not acquire server internals through incidental filesystem paths.

The Rust server package is the destination for page indexing, HTTP serving and
Rspack compilation. Its shipping Rust capability is the page index. HTTP and
compilation currently run under `server/node/`; moving source files does not
establish Rust serving/compiler acceptance. The [preview pipeline](preview-pipeline/README.md)
owns those integration and performance requirements.

## Dependencies and capability layout

The extension composes server and capture entry points. Server workflows use
capture, shared contracts, SDK definitions and UI assets. UI runtime code depends
on browser-safe contracts, never Node, Electron, VS Code or server services.
The SDK stays independent of application hosts. Shared contracts own no sessions,
DOM initialization, filesystem I/O or process lifecycle.

JavaScript/TypeScript capability modules use the owning part's
`src/modules/<capability>/`; current Node services use
`server/node/src/modules/`. Shared UI presentation is `ui/src/components/`,
themes are `ui/src/theme/`, and browser controllers are in `ui/src/`.
Application entry points compose owners. Existing direct capability directories
migrate in bounded steps; a move does not require splitting every function or
introducing speculative shared packages. Rust follows Cargo's standard layout.
The [shared architecture rule](../../../.canonic/rules/module-architecture.md)
and [product guide](../../../.canonic/guides/workbench/AGENTS.md) define the contracts.

Docs illustrate the cross-part boundary: shared parsing, lens and placement rules are
in `contracts/src/docs/`, browser canvas/page modules in `ui/src/modules/docs/`,
and rendering, source reading and portable export services in
`server/node/src/modules/docs/`. The service exposes `index.ts`. Export and
preview-pipeline Node workflows retain their own capability APIs.

## Delivery and verification

The source extension manifest lives in `extension/package.json`; the product
root's private package manifest coordinates tools and Node dependencies.
`stage-extension.cjs` assembles a temporary distribution from the peer owners,
rewrites extension resource paths for that root, includes production dependencies
and selected server binaries, and checks product/extension/SDK versions match.
Cargo sources, build caches, tests, labs and design fixtures do not ship.

Browser modules under `ui/src/` and `contracts/src/` are served with types
stripped on demand. The legacy shell remains at `/_workbench/`; its manifest and
YAML script routes use the shared contract implementation. VS Code webviews
supply local resource roots and CSP for UI assets and shared contracts.

Path migrations update imports, compiler runtime lookup, asset serving, webview
resources/CSP, package assembly, release automation, design pages and source
links together. Checks exercise the extension adapters, real browser interactions,
server routes, native indexing and portable output. Distribution assembly tests
cover manifests, dependencies, binaries and exclusion boundaries. A packaged
launch confirms the assembled paths resolve outside the source tree.

## Rust ownership

The native package follows Cargo's layout, with role-specific submodules and
one public CLI-oriented `run` entry point. Capabilities live directly under `server/src/`.

| Role | Owner and dependency boundary |
| --- | --- |
| CLI composition | `main.rs` owns exit status; `lib.rs` exposes `run`; `cli.rs` parses arguments, transports JSON and supplies the evaluator |
| Index application | `index.rs` coordinates one transaction; `index/definitions.rs`, `cache.rs` and `timings.rs` own input acquisition, cache decisions and diagnostics |
| Pure definition rules | `definition.rs` is the facade; its private compatibility, validation and projection modules use supplied source-existence checks, with no filesystem or process I/O |
| Literal extraction | `literal.rs` interprets conservative SWC syntax without evaluating project code |
| External adapters | `discovery.rs` traverses directories; `evaluate.rs` owns evaluator execution; `saved.rs` owns persistence |
| Answer projection | `protocol.rs` converts shared records into the native response shape |
| Shared data and helpers | `model.rs` owns transaction inputs and observed records; `files.rs` owns filesystem stamps; `paths.rs` owns lexical path operations |

Dependencies flow from the application to rules and adapters. The evaluator
accepts explicit paths/options and exposes typed selection/evaluation methods;
it does not import the index workflow. Discovery and persistence share neutral
record types rather than importing each other's implementation. Generic shared
operations belong in named crate-level helpers; capability-specific helpers
remain private to their owner. No catch-all utilities module is required.

The evaluator adapter owns and reaps its direct child on normal completion and
early failures. The TypeScript host owns the enclosing process group, timeout
and descendant cleanup. Record serialization and the CLI protocol remain
independent of internal module paths. See [page-index acceptance](preview-pipeline/page-index.md#acceptance-and-evidence)
and [code quality](code-quality.md) for the behavioral and engineering checks.
