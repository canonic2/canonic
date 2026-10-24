# Workbench structure and module ownership

## Product parts

**The Workbench UI is independent of the backend. This is a required architecture
boundary, not a description of the current implementation.**

The [rendering-flow contract](rendering-flow.md) defines one Workbench shell and
content-only artboard renderers. Project-preview compilation and static UI
asset delivery are distinct from composing the Workbench interface.

`packages/workbench/` owns the Workbench product. Applications and libraries are
peers; the VS Code extension is one delivery host. The product root coordinates
checks, builds, version consistency and releases, and holds product-wide specs,
user docs, design fixtures and labs.

| Part | Responsibility and boundary |
| --- | --- |
| `extension/` | VS Code activation, commands, webviews, trust input and lifecycle of editor-launched sessions; extension manifest and icons |
| `server/` | The Rust Workbench server in `src/`: HTTP, project services, page index and preview compiler; in `node/`, its host, the standalone command and the Node evaluator |
| `capture/` | Electron screenshot renderer, runtime preparation, platform window capture and native stream pooling |
| `ui/` | Browser canvas, components, themes, preview runtimes/adapters and portable viewer |
| `sdk/` | Published definition helpers and authoring types, independently consumable by projects |
| `contracts/` | Shared manifest/YAML interpretation, wire types and pure rules used across parts |
| `scripts/` | Product build coordination, temporary distribution assembly and release tooling |

The server owns project state, definition discovery, project-preview compilation and output serving
and subordinate compiler processes. The extension owns editor integration and
the sessions it starts. Capture owns rendering resources. Browser controllers
request operations through host contracts; elements render and emit events.
Clients must not acquire server internals through incidental filesystem paths.

The Rust server package is the destination for page indexing, HTTP serving and
Rspack compilation. Its shipping Rust capability is the page index. HTTP and
compilation currently run under `server/node/`; moving source files does not
establish Rust serving/compiler acceptance. The [preview pipeline](../preview-pipeline/README.md)
owns those integration and performance requirements.

## Dependencies and capability layout

The extension composes server and capture entry points. Server workflows use
capture, shared data contracts and SDK definitions. UI runtime code depends
on browser-safe contracts, never Node, Electron, VS Code or server services.
The SDK stays independent of application hosts. Shared contracts own no sessions,
DOM initialization, filesystem I/O or process lifecycle.

### UI and backend independence

- The UI owns the page-list presentation, navigation, artboard rendering and
  arrangement, selection, component state, interaction, themes and UI lifecycle.
  Backend responses provide data and operation results; they do not construct
  Workbench controls or decide how its interface renders.
- The backend exposes project data and operations through explicit API contracts.
  Its domain services must not import UI runtime code, depend on component/DOM
  structure, initialize the shell or decide its presentation. UI asset tooling
  and delivery are host infrastructure, separate from backend domain services.
- The UI depends on an injected data/operation client. HTTP and editor messaging
  adapters implement that client; browser components and capability logic do not
  import backend implementations or require their filesystem layout. A transport
  adapter may use documented same-origin endpoints; its base address is host configuration.
- The browser or editor host loads the UI and supplies its client and host
  capabilities. Static asset delivery is host infrastructure serving opaque
  browser assets. Co-location in an installation, process or origin does not
  authorize application-backend knowledge of UI modules or build conventions.
- Workbench UI source is JavaScript, Web Components, HTML and CSS. Hosts load,
  serve or copy those files unchanged. No TypeScript, transpilation, bundling or
  generated runtime copy participates in UI delivery. Runtime imports, including
  shared browser rules, resolve to JavaScript. Backend declaration files live
  outside the UI and are not browser assets.
- Pinned third-party browser assets live under `ui/src/vendor/` with upstream
  licenses and a hash manifest. Hosts serve those files unchanged; authored UI
  source has no build step. Integrity and the native module graph are checked
  separately from source lint and formatting.
- Compilation, serving and proxying of the project's previews remain backend
  capabilities. The previewed project is content consumed by the Workbench UI;
  its compiler pipeline must not become the Workbench interface's build pipeline.
- Shared contracts contain transport-neutral data, validation and demonstrated
  shared domain rules. They must not introduce backend-to-UI dependencies or
  shared DOM/presentation implementation under another directory name.

#### Acceptance

1. The UI can load and render a supplied page tree and artboard descriptors with
   a fixture client, without starting the Workbench backend or preview compiler.
   Loading, empty, error and interaction states are testable on that boundary.
2. The backend can start and exercise its APIs without the UI source or assets
   installed. Backend domain services do not import UI runtime modules. A generic
   host's opaque UI asset lookup is not a backend application dependency.
   Explicitly selected project files remain preview content, regardless of which
   repository supplies that project; they are not backend application dependencies.
3. UI assets can load from an independent browser/editor host with a configured
   backend client. Replacing the client does not require changing components or
   artboard/page-list rendering code.
4. A UI-only change needs no backend code change or backend rebuild. A backend
   implementation change that preserves the API needs no UI code change.
5. UI unit/component tests use UI-owned fixtures and asset tooling. Separate
   integration tests verify the real API and project-preview pipeline; passing
   those integration tests alone does not establish UI independence.
6. Dependency checks and independent host/backend acceptance tests enforce both
   directions of the boundary. Directory organization or a combined package launch
   alone is insufficient evidence.

#### Current violations and migration scope

The combined host serves authored JavaScript/CSS and the Docs host HTML through
[`files.rs`](../../server/src/serve/files.rs).
The VS Code sidebar loads its JavaScript entry point through an allowed webview
resource URL. Portable docs copy their modules unchanged. No UI transform or
compiler-wrapper dependency is needed by these delivery paths.

Artboards use a dedicated content-only document with explicit metadata,
navigation and annotation/capture ports. Startup request and child-markup checks
cover absence of application initialization; see
[rendering-flow acceptance](rendering-flow.md#acceptance-and-verification-gaps).
Component fixtures load unchanged modules without the backend or compiler.
Complete application loading with a fixture API client and backend API acceptance
without UI files remain unverified; combined-host checks do not prove them.

JavaScript/TypeScript capability modules use the owning part's
`src/modules/<capability>/`; the Node host and evaluator use
`server/node/src/modules/`. Shared UI presentation is `ui/src/components/`,
themes are `ui/src/theme/`, and browser controllers are in `ui/src/`.
Application entry points compose owners. Existing direct capability directories
migrate in bounded steps; a move does not require splitting every function or
introducing speculative shared packages. Rust follows Cargo's standard layout.

Docs illustrate the cross-part boundary: the browser's lens rules are in
`contracts/src/docs/lenses.js`, browser canvas/page modules in
`ui/src/modules/docs/`, and Markdown reading, rendering, source reading and
portable pages in the server's `docs` module ([docs.rs](../../server/src/docs.rs)).

## Delivery and verification

### Browser source ownership

The JavaScript UI is organized by capability, with application startup outside
component folders:

```text
ui/src/
  bootstrap/                 Browser, sidebar and renderer composition/registration
  components/<name>/         Element, styles, owned helpers and interaction tests
  components/shared/         Shared modal presentation styles
  host/                     Browser HTTP transport
  modules/
    canvas/                 Artboard state, geometry, controller and publication
    navigation/             Requested destination, supersession and transition state
    preview-sessions/       Renderer transport, retention, catalog and legacy adapter
    review/                 Capture, reference projection, review workflow and browser delivery
    docs/                   Reading layout and embedded examples
  theme/                    Standalone tokens and host theme assets
```

Canvas, navigation, preview-session and review `index.js` files expose their application contracts. Canvas also has
a pure `presentation.js` entry point for component data and camera geometry;
components cannot import capability operations, bootstraps or host services.
Component styles stay beside their element; shared modal styles have their own
presentation owner. Importing an element class does not register it.

`bootstrap/workbench.js` registers the browser controls and composes the canvas;
`sidebar.js` is the direct webview module entry. `renderer.js` composes the
content-only renderer. Size and docs bridges also initialize from bootstrap.
`bootstrap/canvas.js` connects capability ports to the existing shell. Its
legacy chrome adapter owns the current DOM controls and restores the previous
zoom API when its connection ends.

Navigation owns pending destination and request generations. The preview-session
controller owns the ordinary renderer's parking/expiry policy and shared catalog;
the canvas controller owns artboard instances, geometry and capture locks.
Review owns capture/handoff sequencing and partial failure. Its browser adapter
owns downloads and HTTP delivery. Server-side handoff prompt formatting and its
tests live in the server ([handoff.rs](../../server/src/handoff.rs)), not in browser source.

The outer legacy shell still interprets its configuration, hash and controls.
The content controller separately derives renderer metadata from the project model,
without menu DOM. `bootstrap/renderer.js` composes content, retained frame sessions,
native surfaces and annotation/capture ports inside `content.html`.
The frame lifecycle, native adapter and annotation internals retain JavaScript
factories under `ui/legacy/`; renderer polling and deeper annotation separation
remain bounded legacy debt, along with the docs/sizing acceptance gaps.

### Asset and packaging checks

The source extension manifest lives in `extension/package.json`; the product
root's private package manifest coordinates tools and Node dependencies.
`stage-extension.cjs` assembles a temporary distribution from the peer owners,
rewrites extension resource paths for that root, includes production dependencies
and selected server binaries, and checks product/extension/SDK versions match.
Cargo sources, build caches, tests, labs and design fixtures do not ship.

The combined host serves JavaScript/CSS modules under `ui/src/` and
`contracts/src/` unchanged, plus the authored Docs host HTML at its explicit
module path. Static serving is host infrastructure, independent of
backend domain logic and project compiler lifecycle. The [rendering flow](rendering-flow.md) forbids using that delivery
arrangement to load the full application inside an artboard.
VS Code webviews supply resource roots and CSP for the UI they host.

Path migrations update imports, compiler runtime lookup, asset serving, webview
resources/CSP, package assembly, release automation, design pages and source
links together. Checks exercise the extension adapters, real browser interactions,
server routes, native indexing and portable output. Distribution assembly tests
cover manifests, dependencies, binaries and exclusion boundaries. A packaged
launch confirms the assembled paths resolve outside the source tree.

### Lattice delivery slice

**Status:** planned, not started. Workbench's browser UI will use the Lattice
package (`@canonic2/lattice`) for application state. Before any UI code depends
on it, one narrow change establishes that its modules reach every Workbench
document that will import them. The slice changes module delivery only: no
navigation, canvas or other behavior changes, and no store is used yet.

**Scope:**

- **Dependency.** The product root's `package.json` declares Lattice as a local
  production dependency and keeps its own lockfile.
- **Serving.** The server serves Lattice's runtime files unchanged under a fixed
  path, such as `/_workbench/lattice/`, following the same rules as `ui/src/`:
  JavaScript modules only, tests, declarations and development files refused.
- **Imports.** Each document that will use Lattice imports the existing, empty
  root entry point by its explicit URL. Explicit URLs need no import map, so
  documents without one need no inline script and no CSP change for it. No
  export is added to Lattice for the slice.
- **Documents.** The canvas document (`ui/legacy/index.html` through
  `bootstrap/workbench.js`) and the renderer document (`content.html` through
  `bootstrap/renderer.js`), which are served by the Workbench server and run
  inside the panel webview's frame. The sidebar webview, which loads modules
  through webview resource URLs under its own nonce-based CSP, is added when
  sidebar code adopts Lattice; it then needs a resource root and an import path
  resolved within the staged extension.
- **Packaging.** Distribution assembly copies Lattice's files into the staged
  product as real files. A packaged extension must not contain a symlink or a
  path back into the source checkout.

**Evidence:**

| Check | Establishes |
| --- | --- |
| Server route test | Lattice's runtime files are served byte for byte; tests, declarations and other paths are refused |
| Browser test of both documents | The canvas and renderer documents import the root entry point without errors, from the server, not a development resolver |
| Assembly test | The staged product contains Lattice's runtime files as real files and no symlink or source-checkout path |
| Packaged launch | The installed extension, run without the source checkout, loads both documents and their Lattice import |

The navigation migration in [rendering flow](rendering-flow.md#navigation-state)
starts only after this slice is in place.

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
independent of internal module paths. See [page-index acceptance](../preview-pipeline/page-index.md#acceptance-and-evidence)
and [code quality](code-quality.md) for the behavioral and engineering checks.
