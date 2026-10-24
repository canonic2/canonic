# Developing Workbench

The [user guides](docs/README.md) in `docs/` are published on the website as
they are. The [internal specifications](specs/README.md) in `specs/` define the
behavior the implementation and tests must satisfy; they are not published.
Release notes are in [CHANGELOG.md](CHANGELOG.md).

## Product layout

Workbench has peer owners under one product root: `extension/` for VS Code,
`server/` for Rust (the Workbench server: HTTP, page index and preview
compiler) and `server/node/` for its host, the standalone command and the Node
evaluator,
`capture/` for Electron and platform capture, `ui/` for the browser application,
`sdk/` for the published definition API, and `contracts/` for shared pure rules
and wire data. See [module ownership](specs/system/modules.md).

`ui/legacy/` holds the HTML/script shell and content document; `ui/src/` holds
native JavaScript controllers, Web Components and CSS. Hosts serve or copy these
files unchanged. See [standalone UI use](ui/README.md); project compilation is
separate from UI delivery.

```sh
pnpm install --frozen-lockfile
pnpm test
pnpm run check
pnpm run check:ui
pnpm run test:server
pnpm run serve <folder>
server/bin/<platform>-<arch>/workbench-server check <folder>
```

`pnpm test` builds the debug binary into `server/bin/`. `workbench-server`
also provides `init`, `list` and `build`; see the
[command-line tools](docs/workbench-previews.md#command-line-tools).

The extension manifest is `extension/package.json`. Packaging assembles a
temporary product tree with that manifest, production dependencies, browser
assets, compiler binaries, the Rust executable and Electron bundle. The source
root remains a private development coordinator. Keep its version and the
extension and SDK versions aligned. F5's launch configuration selects
`extension/` as the extension development path.

## Fast development loop

Run these commands from `packages/workbench/`, after the initial frozen
dependency install:

```sh
# Run just the files affected by an edit (optional test-name filter).
pnpm run test:focus ui/src/modules/docs/page/highlighting.test.js
pnpm run test:focus server/node/server.test.js --name 'trace'

# Open this repository in a separate VS Code Extension Development Host.
pnpm run dev ../..
```

`test:focus` requires explicit test files and limits test concurrency to four.
UI-only selections skip Cargo. Other selections first build the debug server,
using Cargo's existing dependency and incremental caches, so tests cannot silently
use an older binary. Test failures stop the command. Select the relevant checks above individually
before completing changes across parts.

`dev` builds the debug server and prepares the local Electron helper, then loads
the extension directly from source. It avoids release compilation, VSIX assembly
and installation. Pass another project folder to try that project's workbench.
Paths are relative to this package. Open this package in VS Code and press F5
for the same preparation under the debugger.

After JS, TypeScript, HTML or CSS edits, run **Developer: Reload Window** in
the development window. After Rust or `capture/app/` edits, first run
`pnpm run dev:prepare`, then reload. That preparation always asks Cargo to check
freshness. The Electron archive is reused only when its checksum, target, pinned
version, helper sources, lockfile and bundler inputs match; changed or damaged
inputs rebuild it. Dependencies and the initial Electron download/build still
need preparation once. Keep `server/target/` between iterations to retain Cargo's
cache; dependency, toolchain and profile changes can require a longer rebuild.

The source extension uses a debug server, so use a release build for performance
measurements. `pnpm test`, the individual Rust verification scripts, release packaging and the repository's
`scripts/install-workbench.mjs` retain their full validation/build paths.

## The two halves

The workbench is one tool in two windows of the editor: the page list in the
sidebar, the canvas in a tab. Neither talks to the other — both talk to
`extension/extension.js`, and what travels between them is the hash the workbench's
address bar would have carried, `pages/sign-in.html:error`:

```text
sidebar  --canonic-pick-->  extension  --wb-go-->    canvas
sidebar  <--canonic-here--  extension  <--wb-here--  canvas
```

So picking a page in the sidebar and pasting a workbench URL are the same
instruction, and following a link inside a live preview leaves the list showing
wherever it ended up.

The list is `ui/legacy/sidebar.html` loaded into a webview, with a `<base>`
saying where that folder is and a `<meta>` saying where the project is — a
webview's address is the editor's, so the page can work out neither by itself.
It paints in VS Code's theme colours rather than the workbench's: it is standing
where a file tree usually does, so it dresses like one.

The canvas is a webview holding one iframe rather than the built-in Simple
Browser, because only the page that owns the frame can pass a pick into it.

`workbenchRoot()` in `extension/extension.js` picks the folder to serve: the first open
folder with a `workbench.yaml`, which is also the `workspaceContains:`
activation event.

## Installing from source

```sh
pnpm install --frozen-lockfile
pnpm run package
code --install-extension canonic-workbench-<target>-<version>.vsix --force
```

The package uses pnpm with a flat (`hoisted`) `node_modules` and runs no
dependency install scripts; `pnpm-workspace.yaml` lists the scripts it skips.
`pnpm run package` writes `canonic-workbench-<target>-<version>.vsix`, such as
`canonic-workbench-darwin-arm64-0.6.0.vsix`, in this folder. Cursor, Windsurf
and the other VS Code forks read their own extensions folder —
`~/.cursor/extensions`, and so on — and their own command line installs the
same file.

After installing, run **Developer: Reload Window** from the Command Palette
in each open VS Code project window. Installing replaces the files on disk;
already-running extension hosts and capture helpers keep their previous code
until their window reloads. Reloading the preview or using **Refresh Pages**
does not restart the extension. This also applies to `scripts/install-workbench.mjs`
at the repository root.

Building and running the extension/backend need Node 24 or later: their TypeScript
loads through Node's type stripping. The browser UI uses JavaScript directly.
The extension requires VS Code 1.123 or later,
the first release on Node 24. `pnpm run package` bundles pinned Electron 44.2.0 and produces a
platform-specific VSIX for the build host. `--target darwin-arm64` (or
`darwin-x64`, `win32-x64`, `win32-arm64`, `linux-x64`, `linux-arm64`) selects
another target. The VSIX carries the production dependencies that
`scripts/production-dependencies.cjs` resolves from `package.json`, rather than
the ones `npm list` reports, which on a pnpm tree include dev-only packages.
`pnpm run package:all` builds every target into `dist/`, one
at a time, finishing with the build host's so its runtime stays in place; name
targets or pass `--out <dir>` to narrow or redirect it.
Use native builders for release validation and signing.
The download happens at build time only. `CANONIC_ELECTRON_ZIP_DIR` can point
to a directory of official Electron ZIPs for an offline build.

Packaging also writes the target's pinned native esbuild binary to `server/node/preview/bin/`.
It copies the matching installed binary, or, when building another target,
downloads the registry archive and checks it against the integrity recorded in
`pnpm-lock.yaml`. The installed Node evaluator uses this bundled
binary; source checkouts use the package dependency. Previews compile with
Rspack's Rust crates inside `workbench-server`, so no Rspack binding ships.
The compiler's persistent cache lives under `canonic-workbench-rspack` in the
host's temporary directory and survives server restarts.

The macOS runtime adds about 135 MB to the VSIX and hundreds of MB to the
installed runtime cache. Its tar archive preserves framework links that VSIX
cannot represent directly. Each archive is verified and extracted atomically
into `globalStorage/capture/<sha256>` on first activation. The first activation
therefore includes extraction; subsequent activations reuse the runtime and
remove runtimes left by other versions, so the cache holds one at a time.
Configure macOS signing/notarization before distributing public releases;
the local development package is not notarized. The `LSUIElement` setting must
be applied before signing. Other desktop targets and remote hosts need
native validation; Linux hosts without a display have no screenshots.

## GitHub builds and releases

The [extension workflow](../../.github/workflows/extension.yml) tests the extension
and builds all six platform VSIX files on matching GitHub-hosted runners when a
`workbench/v<version>` tag is pushed. Bump `version` in `package.json`,
`extension/package.json` and `sdk/package.json`, add a `## <version>` entry to
[CHANGELOG.md](CHANGELOG.md), then push a matching tag (for example,
`workbench/v1.0.1`). The workflow checks the tag against all three versions
and refuses a tag without a changelog entry
(`node scripts/release-notes.cjs <version>` prints the entry, or fails the same
way). It attaches all six VSIX files to a GitHub Release using stable,
versionless asset names, with the changelog entry as its notes. It then
publishes the same files to the Visual Studio Marketplace and Open VSX, publishes
`sdk/` to npm as `@canonic2/workbench`, and publishes the website. Build
files are also available as workflow artifacts for 14 days.
Open VSX publishing uses the `canonic` namespace and GitHub trusted publishing
through OIDC, with `extension.yml` registered as a trusted publisher. Already-published
platform versions are skipped when the Open VSX job is rerun. See
[distribution setup](../../docs/distribution.md#workbench-extension) for account
and credential requirements.
These builds are unsigned; validate and sign platform
releases before treating them as production-ready.

## Capture benchmarks and smoke checks

For capture profiling, run `node packages/workbench/scripts/benchmark-capture.cjs` from
the repository root after bundling the runtime with `pnpm run bundle-runtime`. It runs current helper sources
in an isolated temporary runtime and compares plain, CSS blur, and backdrop-blur
fixtures at mobile and desktop sizes. It reports preparation, native readback,
Retina resizing, PNG encoding, and helper round-trip time, with six on-demand
captures per case, including one after idle. Reports and PNGs remain in the
printed temporary directory; the temporary runtime is removed.

Run `node scripts/smoke-preview-sessions.cjs` from this package to check retained
React, Storybook, and URL iframes in Chrome. It verifies form edits, menus,
scroll, React state, channel reuse, inactive navigation isolation, and Reload.
Unit tests cover idle expiry, eviction, superseded loads, and source revisions.

Pass `<project> <page> [width height]` to profile a particular local page.
Set `CANONIC_BENCH_FORMAT=jpeg` to benchmark the workbench's 90%-quality JPEG
output instead of PNG.
JPEG downsampling uses Electron's intermediate (`better`) resize setting;
`CANONIC_BENCH_RESIZE=best` compares the slower `best` setting. Warm-up primes
pixel readback without encoding a discarded file. The Save button and handoff
complete their capture when the file is written; neither reads the saved image
back into the canvas.
Set `CANONIC_BENCH_CAPTURE=cdp` to compare Chromium's speed-optimized PNG path,
or `CANONIC_BENCH_CAPTURE=dom` for the vendored DOM renderer. These experimental
modes modify only the temporary helper. The CDP comparison attaches to that
helper, never VS Code, and scales its capture to CSS pixels. It uses a different
downsampling path, so compare the saved images as well as timings. The DOM mode
captures the minimal capture wrapper, not the complete interactive workbench.

Run `node packages/workbench/scripts/smoke-mirror.cjs` to exercise live shadow DOM,
forms, scrolling, adopted stylesheets, canvas, dialogs and popovers between two
isolated Electron renderers. It verifies that app scripts do not run in the
mirror and unchanged elements retain their identity. Optional `<project> <page>`
arguments exercise a heading and the first input inside an open shadow root in
a selected project. It saves source/mirror JPEGs and timing reports under a
temporary directory, without modifying project files. These timings separate snapshot,
background preparation and warm capture; they are not VS Code click timings.
Set `CANONIC_MIRROR_WORKBENCH=1` to exercise the complete hosted workbench Save
button, including background synchronization, an immediate property edit, the
HTTP request and disk write. This mode creates a temporary project with links
to the selected source files and its own `.canonic/.handoffs/` directory. It uses Electron
to host the workbench, so it still does not measure VS Code's webview overhead.
`CANONIC_MIRROR_SAMPLES=6` records six captures. Each reports frontend request
preparation, helper queue time, mirror preparation, readback, resizing and encoding.
Reports also include request size, changed node/property counts and whether
the request needed a full snapshot. The default fixture verifies structural
edits, reparenting, cumulative reversions and recovery of the requested revision.
`CANONIC_MIRROR_CDP=1` compares direct Chromium JPEG capture;
`CANONIC_MIRROR_DPR=1` tests a device-scale override only in the temporary receiver.
These are experiments, not production settings; confirm the reported bitmap size
before assuming a scale override removed Retina downsampling.
The `benchmark-capture.cjs` helper round trips include IPC and base64 decoding but exclude saving to disk,
HTTP readback, and contention from an active workbench. A warm benchmark does
not establish which path an installed workbench used during a slow capture.
The built-in temporary fixtures also report one HTTP capture-and-save round trip
per case, including disk saving. Selected projects are never written to by this
benchmark.
Use `CANONIC_BENCH_SAMPLES=1` for a single slow-page sample. DOM runs include
cloning, font embedding, asset embedding, rasterization, and encoding timings.
`CANONIC_BENCH_DOM_STYLES=resolved` experiments with omitting custom properties
from the cloned styles; it does not change the workbench's production renderer.

For a native smoke check, run `pnpm run bundle-runtime`, then
`node scripts/smoke-capture.cjs`. It uses a temporary fixture and checks warm
capture reuse, reloads, resizing, implementation handoffs, Dock visibility,
and crash recovery. Ordinary `pnpm test` needs no desktop or Electron process.

`node scripts/smoke-lenses.cjs` drives an installed Chrome, through the
development driver in `scripts/chrome.cjs`, with temporary app and Storybook
fixtures to check native iframe input, sign-in, session persistence across
reloads and lens changes, and viewport resizing. Set `CHROME_PATH` to choose
the browser. The extension itself never uses Chrome.

`node scripts/smoke-canvas.cjs` exercises multi-artboard support through a
temporary programmatic harness: independent instances, scenario and size changes,
cross-space context, and complete screenshot/handoff payloads. It uses the
same Chrome driver and bundled capture runtime, without adding production UI.

## Individual Rust verification

Run Rust verification categories individually and select only those needed for
the task. A request for tests does not also request formatting, lint, type
checking, doctests, API docs, dependency auditing or coverage. Name the selected
commands before running them; there is no aggregate Rust check target.
`docs` generates browsable HTML API reference in `target/doc/`; `doctest`
executes Rust documentation examples. Run either as its own explicit category.
Rustdoc can recheck dependencies even with `--no-deps`, which only suppresses
their generated reference pages. Keep Cargo caches between runs.

| Category | Command from this package |
| --- | --- |
| format | `pnpm run format:server` |
| format-check | `pnpm run format:check:server` |
| lint | `pnpm run lint:server` |
| types | `pnpm run types:server` |
| test | `pnpm run test:server` |
| doctest | `pnpm run doctest:server` |
| docs | `pnpm run docs:server` |
| deps | `pnpm run deps:server` |
