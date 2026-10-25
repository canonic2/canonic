# Code quality

**Status:** implemented: the browser UI gate, the native Rust gates, Node
integration tests and the workflows that run them. Outside the UI lint/format
baseline: legacy shell JavaScript under `ui/legacy/` and preview adapters;
remaining structure and lifecycle debt is in
[implementation gaps](#implementation-gaps).
Passing these checks is distinct from meeting the
[performance targets](performance.md#measured-interactions) or establishing
installed-host acceptance.

## Scope and ownership

This topic owns tooling, static checks, test conventions and dependency policy.
Feature acceptance belongs to the relevant spec, including
[page-index acceptance](../preview-pipeline/page-index.md#acceptance-and-evidence).
Commands and pinned tool-installation instructions live in the
[native README](../../server/README.md#checks-and-packaging) and the
[development guide](../../DEVELOPMENT.md).

Workbench keeps its own Rust configuration and lockfile; it does not belong to
a shared Cargo workspace, and its releases are not coupled to other packages.

## Development iteration

`test:focus` accepts explicit test files and an optional `--name` filter, with
four concurrent Node test workers. UI-only selections run without Cargo; other
selections require a successful incremental debug build before tests execute.
`dev` and F5 prepare the debug server and local Electron archive, then launch
the authored extension in a VS Code Extension Development Host. Runtime reuse
requires matching source/lockfile/bundler inputs, target and Electron version,
plus verification of the archive checksum. Release packaging still creates a
fresh runtime and release server. These iteration commands do not replace the
full gates below or establish release performance. Commands and reload behavior
are documented in the [development guide](../../DEVELOPMENT.md#fast-development-loop).

## Required gates

### Browser UI

The `check:ui` gate ([development guide](../../DEVELOPMENT.md)) runs these
package scripts in order and stops at the first failure:

| Command | Scope |
| --- | --- |
| `lint` | ESLint recommended JavaScript checks over `ui/src/**/*.js`, browser/host import boundaries, forbidden TypeScript imports, and Web Component rules on elements |
| `format:check` | Prettier over `ui/src` and the UI lint/test tooling; `format` applies the same formatting |
| `test:ui` | Colocated UI pure/controller tests and real-Chrome component interaction tests, plus executable lint-boundary checks |

Chrome or Chromium must be installed; `CHROME_PATH` can select its executable.
The UI gate fails before running tests when no browser is available. It does
not treat skipped browser tests as acceptance. Browser tests use owned temporary
profiles and loopback fixtures, so the gate needs an environment that permits
launching a browser and binding loopback listeners. The existing Node test runner and Chrome harness remain the test infrastructure;
there is no DOM emulator or separate framework-specific component runner.

The UI requires no TypeScript compiler or typed parser. The separate `check` and
`typecheck` commands check backend/extension/capture TypeScript and shared declarations.
Lint, formatting and backend tools remain pinned in the product lockfile.

Production browser code cannot import Node, VS Code, Electron or the server,
extension and capture implementations. Component elements and their helpers
also cannot import capability controllers or legacy shell scripts, or call
the global network APIs. Bootstrap/registration entry points compose dependencies;
tests may import host fixture infrastructure. The pure canvas `presentation.js`
entry supplies component data and geometry. These checks cover explicit imports and
direct global network calls; they cannot prove that every dependency is pure.

The `Workbench UI quality` workflow runs the gate on matching pull requests and
pushes to `main`; extension releases also run it before packaging. Legacy shell
JavaScript under `ui/legacy/` and preview adapters remain outside this UI
lint/format baseline; every document under `ui/src/app/`, the portable viewer
included, is inside it. Their existing product tests still run through `pnpm test`.
Passing the UI gate does not establish architectural conformance of those surfaces.
Component fixtures serve authored JavaScript/CSS directly without backend/compiler
imports, which establishes standalone component hosting, not
[UI/backend independence](modules.md#implementation-gaps). Browser canvas
acceptance rejects application document/module requests inside content and
inspects child markup; [rendering-flow acceptance](rendering-flow.md#acceptance-and-verification)
owns its scope.

### Native server

| Area | Contract |
| --- | --- |
| Toolchain | Pin Rspack's own toolchain for the pinned crates in [rust-toolchain.toml](../../server/rust-toolchain.toml), with rustfmt, Clippy, LLVM tools and the standard library's source; commit `Cargo.lock`, whose Rspack entries follow Rspack's own lockfile at the pinned release |
| Formatting | Use rustfmt defaults |
| Lint | Check every target and feature with Clippy and `-D warnings` |
| Rust policy | Forbid unsafe code; warn on unreachable public items, Clippy pedantic, production unwraps and `mod_module_files`; deny broken and private rustdoc links |
| Tests | Run behavioral tests with nextest and doctests separately; use locked dependencies and all features |
| Documentation | Build dependency-free API documentation, including private items |
| Dependencies | Check advisories, licenses, versions and sources with cargo-deny |
| Coverage | Generate an HTML report with cargo-llvm-cov over nextest; no minimum percentage gate |
| Shipping | Build the target's native release executable with locked dependencies before packaging |
| Host integration | Run the Workbench Node suite and TypeScript check alongside the native gates |

Each Rust verification category is an independent command, listed in the
[development guide](../../DEVELOPMENT.md#individual-rust-verification) and the
[native README](../../server/README.md#individual-rust-verification); there is
no aggregate target. The extension release workflow names each enabled category
in its own step before packaging, with the dependency audit first. API reference
generation is separate from tests and documentation examples. Coverage is also
separate. CI installs the same pinned nextest and cargo-deny tools documented for
local development.

Clippy lint levels belong in `Cargo.toml`; test-only unwrap permission belongs
in `clippy.toml`. Do not suppress broad lint groups or disable
`too_many_lines` to make a gate pass. Legitimate local exceptions require a
specific rationale.

## Behavioral evidence

- Unit tests cover literal extraction, definition validation/projection and
  saved-index behavior. Proptest checks nested/escaped literals and rejection
  of executable values from the literal path.
- Insta records stable page-list JSON alongside explicit behavior assertions.
  Review intended snapshot changes; accepting a snapshot is not verification
  that its behavior is correct.
- Keep one native integration binary at `tests/integration/main.rs`.
  Assert_cmd, assert_fs and predicates exercise the real CLI with temporary
  project and saved-index resources that are cleaned up together.
- Node adapter tests compare the Rust answer against the compiler across
  discovery, evaluation, ordering, validation and failure cases. Server tests
  exercise trust and disabled-preview boundaries.
- Nextest reports every failure rather than stopping at the first. Flag a
  slow test after 30 seconds and terminate it after two minutes.
- Process-heavy suites run separately. They need child processes and signals,
  Node loopback fixtures and cargo-deny's advisory cache; an environment that
  refuses them lacks a prerequisite, which is not a product failure or a
  reason to weaken the check.

Source-level tests, a passing host run, all-platform installation, webview
acceptance and performance measurements are separate evidence. Record missing
behavioral evidence in the owning feature spec rather than treating tooling
parity as completed acceptance.

## Dependency and artifact policy

### Browser and host dependencies

Prefer native browser/platform APIs and small owned modules. The agreed
persistence design uses native IndexedDB in browsers and workspaceState in
VS Code, with no database wrapper dependency. A storage abstraction does not
justify introducing a framework or package for the browser UI.

Workbench's browser assets include locally shipped Lucide icons and the pinned
highlight.js modules used by Docs. They work without a CDN or runtime package
download. Highlighting assets retain their license/manifest and integrity tests;
icons remain included in the extension. New third-party browser dependencies
need a concrete capability/size/lifecycle rationale and local distribution
ownership. This policy does not claim that the native compiler, evaluator or
development toolchain has only those dependencies.

Lattice (`@canonic2/lattice`) is the agreed dependency for application state
([browser UI port](ui-port.md)). Its
rationale: it is a local, zero-dependency package of native ES modules that
hosts serve and package unchanged, with no build step, and it supplies the
store, capability and element contracts the port uses. Its delivery follows
the [Lattice delivery slice](ui-port.md#lattice-delivery-slice).

Keep project framework packages separate from Workbench UI dependencies:
adapters resolve the project's React/Vue/Astro packages under their contracts.
The [hosting contract](../integrations/hosting.md) owns packaged-runtime and
project-dependency prerequisites.

### Rust dependencies and artifacts

cargo-deny's [configuration](../../server/deny.toml) defines the license
allowlist, per-crate license exceptions, advisory ignores, duplicate-version
warnings and the denial of wildcard versions and unknown Git/registry sources.
A license exception names the crates that need it, and each exception group
and advisory ignore states which dependency requires it and why.

Runtime dependencies retain exact manifest pins for reproducible compiler and
parser inputs. Changing a dependency or adding an exception requires reviewing
the resulting lockfile and dependency checks.

Builds, coverage reports and staged platform binaries remain under ignored
`server/target/` and `server/bin/`. Only the selected platform executable enters
the VSIX. Development tooling, source and reports do not ship in it.

## Implementation gaps

Migrated elements keep one directory per component with its local styles,
helper modules, colocated behavior tests and examples, and separate quality
commands. Their boundaries are properties for inputs, composed intent events,
owned Shadow DOM and connection-scoped lifecycle; framework-specific hooks,
JSX lint rules and native-platform runners do not apply.

| Area | Current evidence and limitation |
| --- | --- |
| Component organization | Element folders own presentation, styles and helpers; design pages load shipping implementations. Shared modal styles live under `components/shared/`; startup and registration live under `bootstrap/` |
| Behavior testing | Existing browser tests cover upgrade-time inputs, independent instances, reconnects, events, keyboard/focus and CSP for migrated controls. Canvas interaction also has a separate browser smoke script; this is not complete keyboard/accessibility coverage for every element |
| Registration | Elements register through explicit bootstrap functions, including the export dialog. Browser tests verify importing its class is silent and registration is repeatable |
| Application state | Navigation owns requested destinations and supersession. The content controller derives renderer routes, scenarios, lenses and labels from project data; the outer shell owns its visible controls |
| Renderer isolation | Artboards load `content.html` with a dedicated bootstrap, viewport and annotation/capture ports. Browser assertions reject nested application startup |
| Lifecycle and dependencies | Controllers expose runtime/disposal ports, and migrated elements release connection resources. Global shell APIs, event interception and renderer polling remain integration debt |

Preserve the existing visible controls and warm preview sessions during migration.
Further separation of the frame lifecycle, native surfaces and annotation
internals must preserve destination sizing, docs presentation and capture
behavior. The detailed contracts remain in [modules](modules.md),
[Web Components](../ui/web-components.md), [docs](../pages/docs-pages.md) and
[warm runtime](../preview-pipeline/warm-runtime.md).
