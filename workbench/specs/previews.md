# TypeScript Workbench previews

A Workbench preview renders a component or screen from a project's own source,
in named states, from a `.workbench.ts` or `.workbench.tsx` definition. The
user guide is [Workbench previews](../docs/workbench-previews.md); this spec
records the contract, how it is built, and what is still open. See
[compiler.cjs](../preview/compiler.cjs), [worker.cjs](../preview/worker.cjs),
[browser.js](../preview/browser.js), [server.js](../server.js), and
[manifest.js](../workbench/manifest.js).

The sections below are the agreed contract. Where the code does not yet meet
it, the gap is marked; unresolved choices are under
[open questions](#open-questions).

## Purpose and scope

Previews render project components on the canvas through Workbench's own
compiler, and the design-system export carries them as runnable, server-free
output. They cover HTML, React, Vue, Astro, React Native Web, and
project-registered adapters. They do not run a project's application server,
`astro.config.*`, or middleware; a whole application stays a
[URL lens](implementations.md#url-implementations).

- Previews are discovered by default and open in the native **Workbench** lens.
- Source compiles in a managed worker. Live and portable output share one
  adapter and state lifecycle, and that lifecycle is shared by every built-in
  and custom adapter.
- Managed previews use the two warm frames of the canvas and become ready only
  after mounting, fonts, and visible images.
- The export adds a `browser/` package that opens from a static HTTP server
  without Electron or Workbench; failed builds become warnings while successful
  previews remain; browser build products do not enter source hashes. The CLI
  `build` uses the same builder without a server.

## Workflows

### Author and discover

- A definition default-exports `definePreview({...})` from
  `@canonic/workbench`. Workbench resolves that specifier to its own bundled
  module; the project installs nothing. The framework itself (React, Vue,
  Astro, React Native Web) must come from the project.
- Discovery walks the project for `**/*.workbench.ts` and `**/*.workbench.tsx`,
  in sorted order, skipping dot-prefixed entries, symbolic links, and
  `node_modules`, `dist`, `build`, and `coverage`.
  `previews.include` replaces the patterns; `previews: false` turns previews
  off and closes the worker. Discovery is a file walk in the server process and
  runs no project code.
- A definition that fails to load or validate becomes one problem prefixed
  with its file; other previews still load. A second definition with an
  existing ID fails with `Duplicate preview id`.

### Sidebar placement

- Each preview becomes a screen whose `src` is the definition file. The title's
  first segment names the section, the last the screen, and the segments
  between are joined with ` / ` into one folder. A title without `/` goes in
  **Previews**; a missing title uses the ID. Screens use the `component` icon.
- Imported sections merge into authored sections with the same name.
- An authored screen whose `src` is a definition file keeps its place and
  label and takes the definition's states; the imported duplicate is dropped.
  The file must still match discovery. Viewports: see
  [open question 2](#open-questions).
- **Requirement (decided 2026-10-03):** the sidebar and canvas update when a
  definition file matching discovery is added, removed, renamed, or changes
  its title, states, or viewports, without **Workbench: Refresh Screens**; see
  [the extension's refresh contract](vscode-extension.md#refresh).
  **Gap:** only `workbench.yaml` and `workbench.local.yaml` are watched today,
  so definition changes need **Refresh Screens** or a YAML change.

### Open, states, controls, and actions

- A discovered screen's design lens is the preview itself, labelled
  **Workbench**. Its address is the definition path; `?state=<id>` selects a
  state and the first state is the default. An unknown state renders an error.
- Same-origin `.workbench.ts(x)` addresses load into a warm preview host
  rather than navigating the iframe. A newer load supersedes a queued one, and
  page-level listeners, timers, and animation frames from the outgoing preview
  are released while the compatibility bridge stays installed.
- When ready, the preview reports its inputs, controls, docs, and last 30
  actions to the canvas, which shows **Preview controls**: one field per
  control (`text`, `number`, `boolean`, `select`, `json`), **Reset state**,
  **Actions**, and **Documentation**. Edits re-render the current state with
  input overrides and last until reset, state change, or leaving the screen;
  they never write files or affect references or exports.
- Saving the definition or any file in its dependency graph reloads the open
  preview: in development the page polls a revision URL every second and
  reloads when it changes. This is a full reload, not hot module replacement.
- Astro input edits re-render through the worker; frontmatter runs in Node.

### Compare a design via a `workbench` lens

- An implementation with `kind: workbench` has no `base` or `url`; `start`,
  `catalog`, and a `root` other than `.` are reported. A screen maps it to a preview
  ID. The lens loads the definition path, and a design state loads the preview
  state with the same ID; other states show the preview's first state. The
  preview's source is added to the screen's code pointers for **Open the
  source** and handoffs.
- An unknown ID is reported as `<label>: unknown Workbench preview “<id>”`.
  What the lens then loads is [open question 1](#open-questions).

### Screenshots and handoff

Capture waits for the preview's readiness flag. It fails with the rendering
error, or with `Workbench preview did not finish rendering before capture`
after 8 seconds.

### Export

The [design-system export](export.md) plans one reference per preview state at
the screen's viewports, loading the definition URL with `?state=`. After
references, it asks the worker for the portable build and adds the `browser/`
package and catalog. A preview that fails to build is a warning. **Requirement
(decided 2026-10-03):** a portable build that fails entirely, including a
worker failure, is also a warning, and the export still delivers its sources
and references without `browser/`; see [export](export.md#job-lifecycle).
**Gap:** a worker failure fails the whole export today. The source closure is
the compiler's resolved local module graph (outside `node_modules`); packages
reached through `node_modules` are listed by name and version.

### Portable build and CLI

- The portable build writes each preview's production bundle, an interactive
  viewer (`index.html`, `viewer.js`, `viewer.css`), the canvas's own
  `preview-controls.js`/`.css`, and `workbench.json` (version 1, name,
  previews, warnings). The viewer offers search, State and Viewport menus,
  resizable dimensions, Reload, the shared controls, and **Open preview**;
  its address keeps the selection.
- Astro previews carry every authored state pre-rendered. Their controls are
  removed and a note is appended to their docs, since input edits need a live
  render.
- `preview/cli.cjs` ships in the extension. `init` writes `workbench-env.d.ts`
  and a minimal `workbench.yaml` without overwriting. `check` evaluates and
  compiles every preview and exits nonzero on any failure. `build` refuses a
  nonempty output folder, writes successful previews, and exits nonzero when
  any failed. `check` and `build` honor `previews: false`. The CLI runs
  definitions in its own Node process with no trust check; running it is the
  user's explicit choice.

## System behavior

### Definition contract

- `id`: kebab-case segments separated by `/`, unique per project. `adapter`:
  a kebab-case name. `source.entry`, and any per-state `source.entry`, must
  exist inside the project after resolving symbolic links.
- `states`: a map of kebab-case IDs to objects; absent or empty means one
  `default` state. Labels default to title case.
- `inputs`, `fixtures`, `globals` merge preview, then state, then (for inputs)
  control overrides, and are structured-cloned per render. Validation only
  checks that state values serialize as JSON.
- `viewports`: a nonempty subset of `fit`, `desktop`, `mobile`, `responsive`.
  `controls`: known types; `select` needs nonempty `options`.
- `styles`, `assets`, and `environment` resolve from the definition. Declared
  assets must stay inside the project.
- [api.d.ts](../preview/api.d.ts) types state inputs against preview inputs.

### Adapters and project configuration

- Built-ins: `html`, `react`, `vue`, `astro`, and `react-native-web` (the
  React runtime with `react-native` aliased to `react-native-web` and `.web.*`
  extensions preferred).
- `workbench.config.ts`, or `previews.config`, is evaluated like a definition.
  Its `adapters` map a name to a `runtime` module (`mount(canvas, source,
  context, environment)`) and optional `plugins`; registering a built-in name
  replaces it. It also sets global `plugins`, `aliases`, `dedupe`,
  `resolveExtensions`, and `define`. An unregistered name fails with
  `Unknown adapter`. Plugins are esbuild plugins and run before Workbench's own
  loaders.
- Custom adapters, compiler plugins, live output, and portable output share the
  same entry and [browser runtime](../preview/browser.js).

### Compile worker lifecycle and trust

- Definitions and config are bundled for Node and executed only in a forked
  worker, never in the extension host or capture renderer. The worker serves a
  loopback port and handles one request at a time.
- It starts lazily on the first config resolution that finds definitions in a
  trusted workspace with previews enabled, and must report its port within
  30 seconds or startup fails with a problem. A change to the resolved
  `previews` value replaces it on the next resolution; `previews: false`
  closes it. Catalog reads time out after 120 seconds, exports after 300,
  and proxied requests after 120.
- If it exits unexpectedly, the next preview request or config resolution
  starts a new one; there is no proactive restart.
- It stops with the server, which asks it to close and kills it after
  3 seconds. It also exits when its parent's IPC channel disconnects.
- Its error output is logged as `preview.worker` (each chunk truncated), and
  per-request timing as `preview.request.completed`.
- Compiled output is cached against the size and modification time of every
  file it read; config changes clear the cache. Input-edit renders are not
  cached. Only discovered definitions inside the project compile.
- In an untrusted server (`isTrusted: false`), no definition runs, discovered
  files produce `Workbench previews require a trusted workspace.`, and preview
  routes answer 404. When trust applies is part of the
  [extension's trust contract](vscode-extension.md#workspace-trust).

### Runtime selection

The worker runs on the [bundled Electron runtime](capture.md#bundled-electron-runtime)
in Node mode when that runtime is used on the host, and otherwise on the
current process's executable (Node for the standalone server). `NODE_OPTIONS`
is removed from its environment. Compilation uses `esbuild-wasm`.

### Readiness and errors

- A render aborts the previous signal and runs cleanups in reverse; a failed
  cleanup does not stop the others. `setup` runs environment, preview, then
  state; the source mounts; `play` and `ready` follow; then fonts, then visible
  images (offscreen lazy images excluded) or 3 seconds.
- Thrown errors, unhandled rejections, adapter errors, and `context.error`
  clear readiness, release resources, and show the error in the frame.
  **Reset state** recovers.
- Problems from discovery, validation, worker failure, and lens mapping join
  the config's `problems`; see [core](core.md#problem-reporting).

## Acceptance criteria and verification points

- [preview.test.js](../preview.test.js): discovery, ID and state validation, and
  isolation of invalid definitions; HTML scripts, inline modules, CSS,
  `srcset`, and assets; React and Vue SFCs with project framework versions;
  custom adapters and plugins on the shared path; project escape and excluded
  definitions; server catalog, compiled pages, runnable export, and worker
  close; untrusted and disabled workspaces never executing definitions; manual
  placement (via `mergeSections`); `workbench` lens state mapping; source
  closure hashing; authoring type checks; portable viewer and partial
  success; CLI `build` refusing nonempty output.
- [preview-runtime.test.js](../preview-runtime.test.js): renderer disposal,
  font and image readiness, hidden-frame readiness, lifecycle merging and
  cleanup, failed cleanups, concurrent host commands, async adapter errors.
- [preview-astro.test.js](../preview-astro.test.js): props, states, slots,
  assets, Node-only frontmatter, live input renders, portable states, missing
  dependencies and unsupported integrations, aliases and defines.
- [workbench/preview-host.test.js](../workbench/preview-host.test.js): warm
  host reuse, superseded loads, style retention, listener and timer release,
  refusal of external pages. [workbench/preview.test.js](../workbench/preview.test.js)
  checks mounting into the warm spare frame.
- [preview-scripts.test.js](../preview-scripts.test.js) checks the shared
  compatibility injection.
- Not covered: [manifest.test.js](../workbench/manifest.test.js) and
  [server.test.js](../server.test.js) have no TypeScript-preview cases; no test
  covers an unknown lens ID, manual-placement viewports, definition watching,
  an export that survives a failed portable build, CLI `check` or `init`
  beyond type generation, the worker's timeouts or restart, or packaging.

## Decisions and discoveries

- **WASM compiler.** `esbuild-wasm` keeps the installed extension independent
  of the host CPU without downloading native tools (rationale in
  [compiler.cjs](../preview/compiler.cjs)).
- **Project code stays out of the extension host and capture renderer**; Astro
  frontmatter stays in the worker and only rendered HTML and client assets reach
  the browser ([worker.cjs](../preview/worker.cjs), [astro.cjs](../preview/astro.cjs),
  `preview-astro.test.js`).
- **Hidden frames.** Browsers can suspend animation frames in the hidden spare
  iframe until promotion, which itself waits for readiness, so readiness never
  depends on a paint (comment in [browser.js](../preview/browser.js); test
  "a hidden iframe becomes ready…").
- **Asset requests skip catalog rebuilds** but recheck configuration, trust,
  and settings ([server.js](../server.js)).
- **2026-10-03, reproduction** with the standalone server on a scratch Acme
  fixture: an unknown lens ID produced the documented problem while the
  screen kept `implementations.built = { preview: "components/missing" }` with
  no `path`; a manual screen placing a definition that declares
  `viewports: [mobile]` resolved and merged with all four viewports.
- **2026-10-03, manual CLI check**: an installed local build at
  `~/.vscode/extensions/canonic.canonic-workbench-0.6.0` (from an earlier
  working-tree state; four preview files differ from the current tree) ran
  `check` and `build` on an HTML-adapter fixture with Node 24 on macOS arm64;
  both succeeded and wrote the viewer. No other adapter, platform, or packaged
  VSIX was tried.

## Implementation gaps

- **Possible duplicate workers (hypothesis, from code reading).**
  `importPreviews` in [server.js](../server.js) closes the old worker on a
  `previews` change, then sets `previews = null` after an `await`. Two
  overlapping resolutions after a YAML edit (the sidebar's catalog request and
  the canvas refresh both resolve the config) could each pass that check; the
  later one would then discard the worker the earlier one created without
  closing it, leaving it running until the extension host exits. Not
  reproduced.
- **Idle worker after previews disappear.** When discovery finds no
  definitions, resolution returns before touching the running worker, so it
  keeps running until the server stops or `previews` changes.

## Open questions

1. **Unknown lens ID.** The problem is reported, but the lens stays on the
   screen without a `path`, and the canvas builds the URL as base plus
   `undefined`, so it appears to load `<root>/undefined`. The same happens with
   no problem at all when previews are disabled, untrusted, or none are
   discovered, because mapping is skipped. Should the lens be dropped, or show
   a named error?
2. **Manual-placement viewports.** The merge adopts a preview's viewports only
   when the authored screen has none, but both config readers always fill all
   four, so the definition's `viewports` never apply to a manually placed
   screen, on the canvas or in export. [Core](core.md#canvas-and-frame) says
   omission enables all four; which rule wins for a placed preview?
3. **CLI from an installed extension.** No automated test runs the CLI from a
   packaged or installed VSIX, on other platforms, or with React, Vue, or Astro
   resolving from the user's project. The documented install path assumes a
   folder without a platform suffix, which matched the local install.
4. **JSON vs. cloneable data.** Validation checks that state data serializes
   as JSON; the runtime uses `structuredClone`, and the guide says "plain,
   cloneable data". Which is the contract?
5. **Unexpected worker exit.** Should it be logged and reported as a problem,
   rather than surfacing only as a failed request until the next one restarts
   the worker?
6. **Trust.** Whether the trust gate is reachable at all is an
   [extension question](vscode-extension.md#open-questions).
