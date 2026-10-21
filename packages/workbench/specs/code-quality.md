# Code quality

Status: implemented engineering baselines for the JavaScript UI, Rust page index and
Node integration. Passing these checks is distinct from meeting the
[performance targets](preview-pipeline/performance.md#measured-interactions)
or accepting the compiler-host prototype.

## Scope and ownership

This topic owns tooling, static checks, test conventions and dependency policy.
Feature acceptance belongs to the relevant spec, including
[page-index acceptance](preview-pipeline/page-index.md#acceptance-and-evidence).
Commands and pinned tool-installation instructions live in the
[native README](../server/README.md#checks-and-packaging) and the
[package guide](../AGENTS.md#commands-and-distribution).

The baseline follows the repository's Rust packages, including TUI and
Registry. Workbench keeps its own configuration and lockfile; it does not
introduce a root Cargo workspace or couple product releases.

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
are documented in the [development guide](../DEVELOPMENT.md#fast-development-loop).

## Required gates

### Browser UI

Run `pnpm --dir packages/workbench run check:ui` from the repository root.
The gate runs these commands in order and stops at the first failure:

| Command | Scope |
| --- | --- |
| `lint` | ESLint recommended JavaScript checks over `ui/src/**/*.js`, browser/host import boundaries, forbidden TypeScript imports, and Web Component rules on elements |
| `format:check` | Prettier over `ui/src` and the UI lint/test tooling; `format` applies the same formatting |
| `test:ui` | Colocated UI pure/controller tests and real-Chrome component interaction tests, plus executable lint-boundary checks |

Chrome or Chromium must be installed; `CHROME_PATH` can select its executable.
The UI gate fails before running tests when no browser is available. It does
not treat skipped browser tests as acceptance. Browser tests use owned temporary
profiles and loopback fixtures. Run this gate with host access in an agent sandbox.
The existing Node test runner and Chrome harness remain the test infrastructure;
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
JavaScript, preview adapters and the portable viewer remain outside this UI
lint/format baseline. Their existing product tests still run through `pnpm test`.
Passing the UI gate does not establish architectural conformance of those surfaces.
It also does not establish [UI/backend independence](modules.md#ui-and-backend-independence):
component fixtures serve authored JavaScript/CSS directly without backend/compiler
imports. That proves standalone component hosting; complete application/API
independence still needs its own acceptance.
Browser canvas acceptance rejects application document/module requests inside
content and inspects child markup; that assertion failed against the shell-backed
renderer. [Rendering-flow acceptance](rendering-flow.md#acceptance-and-verification-gaps)
records the remaining independent-host and live adapter verification scope.

### Structure review and remaining gaps

The October 2026 comparison with the reference React UI library examined its
component directories, typed inputs, local styles, helper modules, colocated
behavior tests, examples, and separate quality commands. Workbench's migrated
elements use equivalent boundaries through properties, composed intent events,
owned Shadow DOM and connection-scoped lifecycle; framework-specific React
hooks, JSX lint rules and native-platform runners do not apply.

| Area | Current evidence and limitation |
| --- | --- |
| Component organization | Element folders own presentation, styles and helpers; design pages load shipping implementations. Shared modal styles live under `components/shared/`; startup and registration live under `bootstrap/` |
| Behavior testing | Existing browser tests cover upgrade-time inputs, independent instances, reconnects, events, keyboard/focus and CSP for migrated controls. Canvas interaction also has a separate browser smoke script; this is not complete keyboard/accessibility coverage for every element |
| Registration | Elements register through explicit bootstrap functions, including the export dialog. Browser tests verify importing its class is silent and registration is repeatable |
| Application state | Navigation owns requested destinations and supersession. The content controller derives renderer routes, states, lenses and labels from project data; the outer shell owns its visible controls |
| Renderer isolation | Artboards load `content.html` with a dedicated bootstrap, viewport and annotation/capture ports. Browser assertions reject nested application startup |
| Lifecycle and dependencies | Controllers expose runtime/disposal ports, and migrated elements release connection resources. Global shell APIs, event interception and renderer polling remain integration debt |

Preserve the existing visible controls and warm preview sessions during migration.
The JavaScript modules own navigation, retention and review workflows through
explicit ports; the legacy chrome adapter owns current DOM control wiring.
The content-only renderer preserves retained Storybook sessions and the outer UI.
Frame lifecycle, native surfaces and annotation internals remain bounded legacy
modules; their further separation must preserve destination sizing, docs presentation
and capture behavior. The detailed contracts remain in [modules](modules.md),
[Web Components](web-components.md), [docs](docs-pages.md) and
[warm runtime](preview-pipeline/warm-runtime.md).

### Native server

| Area | Contract |
| --- | --- |
| Toolchain | Pin Rspack's own toolchain for the pinned crates, `nightly-2026-04-16`, with rustfmt, Clippy, LLVM tools and the standard library's source; commit `Cargo.lock`, whose Rspack entries follow Rspack's own lockfile at the pinned release |
| Formatting | Use rustfmt defaults |
| Lint | Check every target and feature with Clippy and `-D warnings` |
| Rust policy | Forbid unsafe code; warn on unreachable public items, Clippy pedantic, production unwraps and `mod_module_files`; deny broken and private rustdoc links |
| Tests | Run behavioral tests with nextest and doctests separately; use locked dependencies and all features |
| Documentation | Build dependency-free API documentation, including private items |
| Dependencies | Check advisories, licenses, versions and sources with cargo-deny |
| Coverage | Generate an HTML report with cargo-llvm-cov over nextest; no minimum percentage gate |
| Shipping | Build the target's native release executable with locked dependencies before packaging |
| Host integration | Run the Workbench Node suite and TypeScript check alongside the native gates |

Rust verification runs through independent server Make targets: `format-check`,
`lint`, `types`, `test`, `doctest`, `docs` and `deps`. Workbench exposes matching
`:server` scripts (format verification is `format:check:server`). The extension
release workflow names each enabled category in its own step before packaging, with the
dependency audit first. Local runs select categories explicitly; API reference
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
- Run process-heavy suites separately. Sandbox access must support child
  processes and signals, Node loopback fixtures, and cargo-deny's advisory
  cache; a sandbox refusal is an unavailable prerequisite, not a product
  failure or a reason to weaken the check.

Source-level tests, a passing host run, all-platform installation, webview
acceptance and performance measurements are separate evidence. Record missing
behavioral evidence in the owning feature spec rather than treating tooling
parity as completed acceptance.

## Dependency and artifact policy

Allow Apache-2.0, MIT and Unicode-3.0 licenses. Ignore the unpublished package's
own license, warn on duplicate versions, deny wildcard versions and unknown
Git/registry sources, and check advisories. Package-specific license
exceptions permit `Apache-2.0 WITH LLVM-exception` for `ar_archive_writer`, used
by SWC's stacker/psm build dependencies; `CC0-1.0` for `notify` and `ISC` for
`inotify` and `inotify-sys`, the file watcher and its Linux backend;
`BSD-3-Clause` for `matchit`, the HTTP router's path matcher; and, one crate
each, the licenses of Rspack's compiler dependencies, the same crates Rspack's
published native binding is built from: `MPL-2.0` for Lightning
CSS and its parsers (`lightningcss`, `lightningcss-derive`, `cssparser`,
`cssparser-color`, `cssparser-macros`, `dtoa-short`, `parcel_selectors`,
`static-self`, `static-self-derive`, `smartstring`), `Zlib` (`foldhash`,
`foldhash-portable`, `libz-rs-sys`, `zlib-rs`, `slotmap`), `ISC` (`hmac-sha256`,
`is_ci`, `libloading`), `BSD-2-Clause` (`Inflector`, `pnp`), `BSD-3-Clause`
(`swc_sourcemap`), `BSD-2-Clause-Patent` (`ustr-fxhash`), `BSL-1.0`
(`xxhash-rust`) and `Apache-2.0 WITH LLVM-exception` (`dragonbox_ecma`); and
`ISC` for `ring`, `rustls-webpki` and `untrusted` and `BSD-3-Clause` for
`subtle`, the TLS the implementation proxies use; and `BSD-3-Clause` for
`encoding_rs`, which the design export's PDF library reads text encodings with. The
advisory RUSTSEC-2026-0235 is ignored for `rkyv` 0.7, which only Lightning
CSS's source-map crate uses, on archives it wrote itself; 0.7 has no fixed
release.

Runtime dependencies retain exact manifest pins to align parser inputs with
the planned compiler-host evaluation. Test dependencies use the same tools
and feature settings as the peer Rust packages. Changing a dependency or adding
an exception requires reviewing the resulting lockfile and dependency checks.

Builds, coverage reports and staged platform binaries remain under ignored
`server/target/` and `server/bin/`. Only the selected platform executable enters
the VSIX. Development tooling, source and reports do not ship in it.
