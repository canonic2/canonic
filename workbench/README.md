# Workbench

By Canonic. A design workbench for the screens already in your repo. Every page and preview
on one canvas, at a real device width — draw on one and hand the picture to your
agent.

![The workbench, showing a sign-in page at desktop width](media/workbench.png)

Screens come from three places, and one canvas shows them all:

- **TypeScript previews.** Define components and screens in `.workbench.ts`
  files with the HTML, React, Vue, Astro, or React Native Web adapter, or one
  of your own. Workbench discovers them, renders their named states, and gives
  each one input controls, reset, an action log, and docs.
- **HTML pages** you list in `workbench.yaml`, with states keyed off
  `html[data-wb-state]`.
- **Implementation catalogs**: a Storybook's stories, or the booted iOS
  Simulators.

The [documentation](https://canonic.sh/workbench/docs/) covers setup, pages
and states, TypeScript previews, lenses, Storybook, the iOS Simulator, app
windows, markup and handoffs, export, and every `workbench.yaml` key.

## Quick start

1. Install the extension.
2. Write `workbench.yaml` at the root of your project:

   ```yaml
   name: Acme

   sections:
     - name: Pages
       icon: file-text
       items:
         - label: Sign in
           src: pages/sign-in.html
           states:
             - id: default
               label: Default
             - id: error
               label: Wrong password

     - name: Components
       icon: component
       items:
         - label: Button
           src: components/button.html
   ```

   With [TypeScript previews](https://canonic.sh/workbench/docs/workbench-previews/),
   `name: Acme` alone is enough: the previews supply the screen list.

3. Open the folder. **Workbench** appears in the activity bar, and opens the
   canvas.

Your pages need nothing added to them — no script tags, no imports, no folder
copied into the repo. The extension serves the workbench alongside your project
and puts what a preview needs into the pages it serves. Delete `workbench.yaml`
and every trace of this is gone. The
[getting started guide](https://canonic.sh/workbench/docs/getting-started/)
walks through it.

## What you get

- **The screen list in the sidebar**, where a file tree usually goes: sections
  listed above the chosen one's screens, folders, and the states of a screen under it.
- **States.** One screen, several versions — the empty form and the one that
  came back wrong. A TypeScript preview declares its states with their inputs;
  an HTML page answers to `html[data-wb-state]`. See
  [pages and states](https://canonic.sh/workbench/docs/pages-and-states/).
- **Real widths.** Fit, desktop, mobile, or a frame you drag, with Figma-style
  zoom and pan on the canvas.
- **Markup and screenshots.** Scribble, arrows, shapes, text, comments. The
  camera downloads a JPEG; handoffs persist their image in the project's
  ignored `.canonic/.handoffs/` folder.
- **Handoff.** The screenshot is saved, the markup is cleared, and a written
  account of every mark is copied to the clipboard for any conversation.
- **Actions off by default**, so clicking around a screen you're reviewing
  doesn't navigate you out of it. Turn it on to walk the real flow.
- **Implementation lenses.** The same screen as a TypeScript preview, a
  Storybook story, a page on the dev server or staging, the iOS Simulator, or
  any macOS app's window — one click away from the design, in the same frame,
  with the same marks over it. See below.
- **Design-system export.** One ZIP with every screen's design and source
  files, reference screenshots, and a standalone browser viewer of the
  TypeScript previews. See
  [design-system export](https://canonic.sh/workbench/docs/design-system-export/).
- **Bounded diagnostics.** Browser, capture, server, and handoff failures meet
  in VS Code's session-managed **Workbench** log, never in the project. Each
  server stops logging after 2 MB in one session, and records never contain
  screenshots, page HTML, or handoff prompts.

## Implementation lenses

A design screen is one picture of a thing that also exists as code. The
workbench can show that code's picture in the design's place: declare the
implementations once, then say where each screen is in each of them.

```yaml
implementations:
  storybook:
    kind: storybook
    url: http://localhost:6006
    root: ../product/packages/ui        # where that code lives, for the handoff
  dev:
    kind: url
    base: http://localhost:3710
    root: ../product
  staging:
    kind: url
    base: https://staging.example.com

sections:
  - name: Pages
    items:
      - label: Sign in
        src: pages/sign-in.html
        states:
          - id: default
            label: Default
          - id: error
            label: Wrong password
        implementations:
          dev:                            # a map of this screen's state ids to paths…
            default: /
            error: /?error=1
          staging: /                      # …or one path for every state
        code:
          dev: packages/auth/src/pages/login

  - name: Components
    items:
      - label: Button
        src: components/button.html
        implementations:
          storybook: Components/Button    # a story title, as Storybook shows it
```

A screen that declares any of them gets a **lens switcher** in the toolbar:
Design, then each implementation it has. The choice sticks as you move
between screens, like the width does; a screen without that lens shows its
design. The address carries it — `#pages/sign-in.html:error@393~staging` —
so a copied link means the implementation too.

Storybook and URL lenses load in an iframe, so typing, scrolling, and sign-in
happen in the page itself; the app must allow being embedded. A `workbench`
lens shows one of the project's TypeScript previews by its ID, and
`ios-simulator` and `window` lenses stream a native window. Code pointers
resolve against each implementation's `root`: the `</>` button opens them in
the editor, and the handoff quotes their absolute paths, so the agent edits the
implementation the picture is of.

Ports, and where the product's code is on your disk, belong in
`workbench.local.yaml` beside the committed file. It is merged over
`workbench.yaml` — each implementation by name, everything else whole — and
belongs in `.gitignore`. See
[lenses and URL implementations](https://canonic.sh/workbench/docs/lenses/)
and the [workbench.yaml reference](https://canonic.sh/workbench/docs/configuration/).

## Commands

| Command | Does |
| --- | --- |
| `Workbench: Open Canvas` | the canvas, in an editor tab |
| `Workbench: Open Canvas in Browser` | the same thing, in your browser |
| `Workbench: Copy Canvas URL` | the address, for a bookmark or a script |
| `Workbench: Refresh Screens` | re-read the config and find TypeScript previews again |
| `Workbench: Show Log` | the Workbench log, for failures from the browser, capture, server, and handoffs |

## Settings

`canonic.capture.chromePath` — an explicit Chrome, Chromium, or Edge executable
for fallback screenshots. Packaged desktop builds take screenshots with a
bundled background Electron helper, with no visible window or Dock icon and no
VS Code startup flags. An installation without one, or a helper that cannot
unpack or start, uses Chrome instead. Leave the setting empty to detect Chrome
from its standard location when it is needed. See
[the VS Code extension](https://canonic.sh/workbench/docs/extension/) for the
server, its ports, the screenshot helper, and workspace trust.

---

## Working on it

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
| `screens.js` | the Workbench view in the sidebar |
| `startup.js` | checks configured implementations and starts their `start` commands |
| `server.js` | the HTTP server, capture, export, preview and lens endpoints, and script injection — plain node |
| `preview-scripts.js` | the `preview-compat.js` bundle (`keys.js`, `actions.js`, `states.js`) injected into served pages |
| `preview-service.js` + `preview/` | the TypeScript preview worker: discovery, compilation, adapters, the browser runtime, the portable viewer, and the `cli.cjs` command line |
| `export.js` | the design-system ZIP export |
| `electron-capture.js` + `capture-helper/` | bundled background screenshot renderer, preparation and crash recovery |
| `window-stream.js` + `window-capture/Capture.swift` | native ScreenCaptureKit and VideoToolbox stream of one app window, for the iOS Simulator and window lenses |
| `electron-runtime.js` | verifies and unpacks the bundled Electron runtime, shared by capture and the preview worker, once into extension storage |
| `capture.js` | Chromium over a private DevTools pipe for fallback screenshots |
| `remote.js` | fetches the configured Storybook's index |
| `config.js` + `yaml.js` | reads `workbench.yaml` and `workbench.local.yaml` on this machine: absolute paths, implementation origins |
| `handoff.js` | composes the prompt an agent is handed. Pure string work |
| `workbench/` | the tool itself — see [its README](workbench/README.md) |

`server.js`, `remote.js`, `handoff.js`, `config.js` and `yaml.js`
never import `vscode`, so they run and are tested without an editor:

```sh
npm test                                     # config parsing, prompt wording, routes, previews, the browser driver
npm run serve -- <folder>                    # the same server the extension runs, on its own
node handoff.js                              # print the prompt for a sample canvas
node config.js <folder>                      # what this machine resolves the config to
node preview/cli.cjs check <folder>          # discover and build every TypeScript preview
```

`preview/cli.cjs` also has `init` and `build`; see the
[command-line tools](docs/workbench-previews.md#command-line-tools).

### The two halves

The workbench is one tool in two windows of the editor: the screen list in the
sidebar, the canvas in a tab. Neither talks to the other — both talk to
`extension.js`, and what travels between them is the hash the workbench's
address bar would have carried, `pages/sign-in.html:error`:

```text
sidebar  --canonic-pick-->  extension  --wb-go-->    canvas
sidebar  <--canonic-here--  extension  <--wb-here--  canvas
```

So picking a screen in the sidebar and pasting a workbench URL are the same
instruction, and following a link inside a live preview leaves the list marked
on wherever it ended up.

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

### Installing from source

```sh
npm ci --ignore-scripts
npm run package
code --install-extension canonic-workbench-<target>-<version>.vsix --force
```

`npm run package` writes `canonic-workbench-<target>-<version>.vsix`, such as
`canonic-workbench-darwin-arm64-0.6.0.vsix`, in this folder. Cursor, Windsurf
and the other VS Code forks read their own extensions folder —
`~/.cursor/extensions`, and so on — and their own command line installs the
same file.

After installing, run **Developer: Reload Window** from the Command Palette
in each open VS Code project window. Installing replaces the files on disk;
already-running extension hosts and capture helpers keep their previous code
until their window reloads. Reloading the preview or using **Refresh Screens**
does not restart the extension. This also applies to `scripts/install-workbench.mjs`
at the repository root.

Building requires Node 22.12 or later; the extension host supports Node
18 or later. `npm run package` bundles pinned Electron 44.2.0 and produces a
platform-specific VSIX for the build host. `-- --target darwin-arm64` (or
`darwin-x64`, `win32-x64`, `win32-arm64`, `linux-x64`, `linux-arm64`) selects
another target. `npm run package:all` builds every target into `dist/`, one
at a time, finishing with the build host's so its runtime stays in place; name
targets or pass `--out <dir>` to narrow or redirect it.
Use native builders for release validation and signing.
The download happens at build time only. `CANONIC_ELECTRON_ZIP_DIR` can point
to a directory of official Electron ZIPs for an offline build.

The macOS runtime adds about 135 MB to the VSIX and hundreds of MB to the
installed runtime cache. Its tar archive preserves framework links that VSIX
cannot represent directly. Each archive is verified and extracted atomically
into `globalStorage/capture/<sha256>` on first activation. The first activation
therefore includes extraction; subsequent activations reuse the runtime and
remove runtimes left by other versions, so the cache holds one at a time.
Configure macOS signing/notarization before distributing public releases;
the local development package is not notarized. The `LSUIElement` setting must
be applied before signing. Other desktop targets and remote hosts need
native validation; Linux hosts without a display use the Chrome fallback.

### GitHub builds and releases

The [extension workflow](../.github/workflows/extension.yml) tests the extension
and builds all six platform VSIX files on matching GitHub-hosted runners when a
`workbench/v<version>` tag is pushed. Bump `version` in `package.json` and its
lockfile, add a `## <version>` entry to [CHANGELOG.md](CHANGELOG.md), then push
a matching tag (for example, `workbench/v1.0.1`). The workflow checks the tag
against the package version and refuses a tag without a changelog entry
(`node scripts/release-notes.cjs <version>` prints the entry, or fails the same
way). It attaches all six VSIX files to a GitHub Release using stable,
versionless asset names, with the changelog entry as its notes, and then
publishes the website. Build files are also available as workflow artifacts
for 14 days.
These builds are unsigned; validate and sign platform
releases before treating them as production-ready.

### Capture benchmarks and smoke checks

For capture profiling, run `node packages/workbench/scripts/benchmark-capture.cjs` from
the repository root after bundling the runtime with `npm run bundle-runtime`. It runs current helper sources
in an isolated temporary runtime and compares plain, CSS blur, and backdrop-blur
fixtures at mobile and desktop sizes. It reports preparation, native readback,
Retina resizing, PNG encoding, and helper round-trip time, with six on-demand
captures per case, including one after idle. Reports and PNGs remain in the
printed temporary directory; the temporary runtime is removed.

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

For a native smoke check, run `npm run bundle-runtime`, then
`node scripts/smoke-capture.cjs`. It uses a temporary fixture and checks warm
capture reuse, reloads, resizing, implementation handoffs, Dock visibility,
and crash recovery. Ordinary `npm test` needs no desktop or Electron process.

`node scripts/smoke-lenses.cjs` uses an installed Chrome with temporary app and
Storybook fixtures to check native iframe input, sign-in, session persistence
across reloads and lens changes, and viewport resizing.

## Licence

MIT. `workbench/modern-screenshot.js` is vendored from
[modern-screenshot](https://github.com/qq15725/modern-screenshot) (MIT, ©
qq15725).

The bundled Electron runtime includes its MIT license and Chromium's third-party
notices in `electron-runtime/runtime.tar.gz`, preserved in the extracted cache.
