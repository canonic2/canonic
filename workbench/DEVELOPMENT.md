# Developing Workbench

The [user guides](docs/README.md) in `docs/` are published on the website as
they are. The [internal specifications](specs/README.md) in `specs/` define the
behavior the implementation and tests must satisfy; they are not published.
Release notes are in [CHANGELOG.md](CHANGELOG.md).

The workbench itself is `workbench/`, and it ships in the `.vsix`. It is plain
HTML and script files with no build step; opening
`workbench/index.html?root=file:///path/to/a/project/` in a browser started with
`--allow-file-access-from-files` is enough to work on the tool.

| File | Job |
| --- | --- |
| `extension.js` | activation, the server's lifecycle, commands, clipboard handoff |
| `panel.js` | the canvas: one webview holding one iframe |
| `sidebar-view.js` | the Workbench view in the sidebar |
| `startup.js` | checks configured implementations and starts their `start` commands |
| `server.js` | the HTTP server, capture, export, preview and lens endpoints, and script injection — plain node |
| `preview-scripts.js` | the `preview-compat.js` bundle (`keys.js`, `actions.js`, `states.js`) injected into served pages |
| `preview-service.js` + `preview/` | the TypeScript preview worker: discovery, compilation, adapters, the browser runtime, request mocks, the portable viewer, and the `cli.cjs` command line |
| `src/modules/export/` | the TypeScript design-system source and ZIP exporter; `index.ts` is its public Node API |
| `electron-capture.js` + `capture-helper/` | bundled background screenshot renderer, preparation and crash recovery |
| `capture-scripts.js` | scripts the screenshot helper runs in a page: the annotation overlay, element descriptions, and export settling |
| `window-stream.js` + `window-capture/Capture.swift` | native ScreenCaptureKit and VideoToolbox stream of one app window, for the iOS Simulator and window lenses |
| `electron-runtime.js` | verifies and unpacks the bundled Electron runtime, shared by capture and the preview worker, once into extension storage |
| `remote.js` | fetches the configured Storybook's index |
| `config.js` + `yaml.js` | reads `workbench.yaml` and `workbench.local.yaml` on this machine: absolute paths, implementation origins |
| `handoff.js` | composes the prompt an agent is handed. Pure string work |
| `workbench/` | the tool itself — see [its README](workbench/README.md) |

`server.js`, `remote.js`, `handoff.js`, `config.js` and `yaml.js`
never import `vscode`, so they run and are tested without an editor:

```sh
pnpm test                                    # config parsing, prompt wording, routes, previews, the browser driver
pnpm run check                               # type-check the TypeScript in src/
pnpm run serve <folder>                      # the same server the extension runs, on its own
node handoff.js                              # print the prompt for a sample canvas
node config.js <folder>                      # what this machine resolves the config to
node preview/cli.cjs check <folder>          # discover and build every TypeScript preview
```

`preview/cli.cjs` also has `init` and `build`; see the
[command-line tools](docs/workbench-previews.md#command-line-tools).

## The two halves

The workbench is one tool in two windows of the editor: the page list in the
sidebar, the canvas in a tab. Neither talks to the other — both talk to
`extension.js`, and what travels between them is the hash the workbench's
address bar would have carried, `pages/sign-in.html:error`:

```text
sidebar  --canonic-pick-->  extension  --wb-go-->    canvas
sidebar  <--canonic-here--  extension  <--wb-here--  canvas
```

So picking a page in the sidebar and pasting a workbench URL are the same
instruction, and following a link inside a live preview leaves the list showing
wherever it ended up.

The list is `workbench/sidebar.html` loaded into a webview, with a `<base>`
saying where that folder is and a `<meta>` saying where the project is — a
webview's address is the editor's, so the page can work out neither by itself.
It paints in VS Code's theme colours rather than the workbench's: it is standing
where a file tree usually does, so it dresses like one.

The canvas is a webview holding one iframe rather than the built-in Simple
Browser, because only the page that owns the frame can pass a pick into it.

`workbenchRoot()` in `extension.js` picks the folder to serve: the first open
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

Building and running need Node 24 or later: the TypeScript in `src/` loads
through Node's type stripping, so the extension requires VS Code 1.123 or later,
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

Packaging also writes the target's pinned native esbuild binary to `preview/bin/`.
It copies the matching installed binary, or, when building another target,
downloads the registry archive and checks it against the integrity recorded in
`pnpm-lock.yaml`. The installed worker uses this bundled
binary; source checkouts use the package dependency. Preview build artifacts
live under `canonic-workbench-previews` in the host's temporary directory and
survive worker restarts. Live source maps are served separately from scripts.

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

The [extension workflow](../.github/workflows/extension.yml) tests the extension
and builds all six platform VSIX files on matching GitHub-hosted runners when a
`workbench/v<version>` tag is pushed. Bump `version` in `package.json`,
`preview/package.json`, and the lockfile, add a `## <version>` entry to
[CHANGELOG.md](CHANGELOG.md), then push a matching tag (for example,
`workbench/v1.0.1`). The workflow checks the tag against both package versions
and refuses a tag without a changelog entry
(`node scripts/release-notes.cjs <version>` prints the entry, or fails the same
way). It attaches all six VSIX files to a GitHub Release using stable,
versionless asset names, with the changelog entry as its notes. It then
publishes the same files to the Visual Studio Marketplace and Open VSX, publishes
`preview/` to npm as `@canonic2/workbench`, and publishes the website. Build
files are also available as workflow artifacts for 14 days.
Open VSX publishing uses the `canonic` namespace and requires the `OVSX_PAT`
Actions repository secret for an account with publishing access. Already-published
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
temporary programmatic harness: independent instances, state and size changes,
cross-space context, and complete screenshot/handoff payloads. It uses the
same Chrome driver and bundled capture runtime, without adding production UI.
