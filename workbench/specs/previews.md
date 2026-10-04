# TypeScript Workbench previews

A Workbench preview renders a component or page from a project's own source,
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

- **Requirement (decided 2026-10-03):** a preview is a mock of its page, in
  the way a Storybook story is, not a running copy of the application.
  Workbench owns everything that would navigate away from it: links and forms
  open other previews through the canvas or are recorded as actions, and the
  Actions switch governs both. Nothing in a preview navigates the frame or
  starts a download. See [links and navigation](#links-and-navigation).
- Previews are discovered by default and open in the native **Workbench** lens.
- Source compiles in a managed worker. Live and portable output share one
  adapter and state lifecycle, and that lifecycle is shared by every built-in
  and custom adapter.
- Managed previews use retained iframe sessions and become ready only
  after mounting, fonts, and visible images.
- The export adds a `browser/` package that opens from a static HTTP server
  without Electron or Workbench; failed builds become warnings while successful
  previews remain; browser build products do not enter source hashes. The CLI
  `build` uses the same builder without a server.

## Workflows

### Author and discover

- A definition default-exports `definePreview({...})` from
  `@canonic2/workbench`. Workbench resolves that specifier to its own bundled
  module, even when the project installs the package, so builds never depend
  on the installed version. The framework itself (React, Vue,
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

- Each preview becomes a page whose `src` is the definition file. The title's
  first segment names the collection, the last the page, and the segments
  between are joined with ` / ` into one group. A title without `/` goes in
  **Previews**; a missing title uses the ID. Page icon precedence is definition
  `icon`, longest `previews.icons` title prefix, configured `previews.icon`,
  then `component`. Prefixes match complete `/` segments, including an exact
  title. Definition icons do not set collection icons. Invalid definition icon
  syntax isolates that definition; invalid configuration icon values are
  reported and ignored. Validation checks kebab-case syntax, not Lucide membership.
- Collection icons resolve independently against the collection name. Highest
  priority first: explicit authored icon, preview prefix mapping, catalog
  prefix mapping, configured preview fallback, configured catalog fallback,
  built-in default. Shared preview/Storybook collections default to
  `component`. Conflicting catalogs at equal priority use lexical icon-name
  order; import order cannot change the selected icon. The shared manifest
  owns prefix matching and collection icon merging, including provenance
  priorities. An authored collection with no explicit icon retains its
  `file-text` default against imported built-in defaults; mappings and
  configured fallbacks win.
- Authored collections with only `name` and `icon` survive both config
  readers and configuration-editor saves. They style imported pages in that
  collection and remain hidden if imports leave them empty, including failed
  catalogs.
- Imported collections merge into authored collections with the same name.
  Within a collection, groups with exactly the same name merge across authored
  pages, previews, and implementation catalogs. The first group keeps its
  position and metadata; later pages append in source order. Matching names
  in different collections remain separate, as do names with different casing.
- An authored page whose `src` is a definition file keeps its place and
  label and takes the definition's states and icon unless it has an explicit
  authored icon; the imported duplicate is dropped.
  The file must still match discovery. Viewports: see
  [open question 2](#open-questions).
- **Requirement (decided 2026-10-03):** the sidebar and canvas update when a
  definition file matching discovery is added, removed, renamed, or changes
  its title, states, or viewports, without **Workbench: Refresh Pages**; see
  [the extension's refresh contract](vscode-extension.md#refresh).
  **Gap:** only `workbench.yaml` and `workbench.local.yaml` are watched today,
  so definition changes need **Refresh Pages** or a YAML change.

### Open, states, controls, and actions

For the [multiple-artboard support system](multiple-artboards.md), each instance
owns its renderer, requested/settled view, controls, action log, and pending-load
generation. Two copies of the same preview must coexist without sharing a
mutable iframe; content navigation updates its originating instance. Shared
origin storage/backend state remains external state. The
[code plan](multiple-artboards-code-plan.md) reuses compiler and framework mount
contracts through explicit runtime adapters. The renderer behavior below
applies independently within each isolated artboard.

- A discovered page's design lens is the preview itself, labelled
  **Workbench**. Its address is the definition path; `?state=<id>` selects a
  state and the first state is the default. An unknown state renders an error.
- Same-origin `.workbench.ts(x)` addresses load into a warm preview host
  rather than navigating the iframe. A newer load supersedes a queued one, and
  page-level listeners, timers, and animation frames remain owned by that
  session until it is evicted or reset. The canvas retains up to three inactive
  sessions for 15 minutes; return checks the compiled revision before reuse.
- Body nodes created during a preview module's import, such as SVG symbol
  sprites, are retained by compiled module URL and restored before its next
  mount. They are detached while another module is active or the host is reset.
  Nodes created during mounting, including portals, are cleared on reset or eviction.
  Imports that finish after being superseded still retain their nodes for a
  later return; a failed mount does not discard import-time nodes.
- When ready, the preview reports its inputs, controls, docs, and last 30
  actions to the canvas, which shows **Preview controls**: one field per
  control (`text`, `number`, `boolean`, `select`, `json`), **Reset state**,
  **Actions**, and **Documentation**. Edits re-render the current state with
  input overrides and last until reset, reload, source revision change, or session eviction;
  they never write files or affect references or exports.
- Saving the definition or any file in its dependency graph reloads the open
  preview: in development the page polls a revision URL every second and
  reloads when it changes. This is a full reload, not hot module replacement.
- Astro input edits re-render through the worker; frontmatter runs in Node.

### Links and navigation

- `links` maps addresses to targets: a preview ID, `{ preview, state }`, or
  `{ state }` for the same preview. Validation rejects a non-map and targets
  that are neither; the catalog carries each target normalized to
  `{ preview, state? }`. A target naming an unknown preview or state is a
  problem (`<file>: link <href> names unknown Workbench preview “<id>”.`, or
  `… names unknown state “<state>” of <id>.`); the other previews still load.
- While a preview is mounted, the runtime ([browser.js](../preview/browser.js))
  registers `wbPreviewActions.follow`, which [actions.js](../workbench/actions.js)
  calls for every link click and form submit with actions on, after in-page
  fragments and before its `.html` hand-off. A key matches when the link and
  the key, both resolved against `document.baseURI`, have the same origin,
  path, and query; the fragment is ignored.
- A match posts `{ type: 'workbench-preview', event: 'navigate', id, preview,
  state }` to the parent. The canvas finds the page whose resolved preview
  has that ID, reveals it, and routes to it with the state (unknown states fall
  back to the default); an ID with no page is ignored. The portable viewer
  selects the preview from its catalog.
- No match: the click or submit is prevented and logged through the context's
  `action`: `navigate` with the raw `href`, or `submit` with the raw `action`
  (omitted when empty) and the form's fields as an object (files by name).
- `context.navigate(to)` takes the same targets, does nothing with actions
  off, and without a parent frame logs `navigate` with `<id>[?state=<state>]`.
- A portable page has no `actions.js`; the runtime installs its own capture
  listeners and behaves as actions on, leaving fragment links to the browser.
- With actions off, `actions.js` stops links and forms before the runtime sees
  them, and nothing is logged.

### Data: request mocks and the project environment

- **Requirement (decided 2026-10-03):** a preview supplies the data its real
  page loads, in whatever way that page loads it, for every adapter:
  `inputs` for props, an environment for providers and stores, `requests` for
  the page's own network calls, and `aliases` for modules. Projects can run
  any code in environments and hooks; `requests` is the built-in tool for the
  common case.
- `requests` (preview and state) maps keys `[METHOD] /path [Operation]` or a
  full URL to a response object or a function `(request, context)`.
  Validation checks key shape and value type. Matching
  ([requests.js](../preview/requests.js)): the state's map before the
  preview's, declaration order within each; a bare path matches any origin;
  `*` matches any characters; a key's query parameters must be present with
  those values; the operation is the body's `operationName`, or the name in
  its `query`, or the same from the query string.
- In the browser, `requests.js` is the first import of every compiled preview
  and replaces `window.fetch` and `XMLHttpRequest` on evaluation, before any
  project module runs. `boot` activates the render's maps before `setup` and
  deactivates them on shutdown. With no maps active, both pass through.
  `/_workbench/` requests on the page's origin always pass through.
- With maps active, an unmatched request answers 404 with a JSON body naming
  it and logs `request` `<METHOD> <path> — no mock`. Writes (methods other
  than GET, HEAD, OPTIONS; for GraphQL, mutations) log `request` with the
  label and parsed body. `pending` waits until the caller's or the render's
  signal aborts; `failed` rejects with a `TypeError` (XHR: `error` event);
  `passthrough` sends the original request. The Actions switch does not
  affect requests.
- Astro: the compiler swaps `globalThis.fetch` for the state's maps around
  each server render, relative URLs resolving against `http://localhost/`,
  with a 10-second abort. Unmatched requests are not logged there.
- `workbench.config.ts` `environment` (a path from the project root, inside
  the project, or a map from adapter name to such a path, so React and Vue
  previews get environments of their own; an unlisted adapter gets none) is imported into every compiled preview and composed with the
  preview's by `combine` in [browser.js](../preview/browser.js): project
  `setup`/`mount` first with cleanups last, `ready` and `configure` in the
  same order, and `wrap` outermost.

### Compare a design via a `workbench` lens

- An implementation with `kind: workbench` has no `base` or `url`; `start`,
  `catalog`, and a `root` other than `.` are reported. A page maps it to a preview
  ID. The lens loads the definition path, and a design state loads the preview
  state with the same ID; other states show the preview's first state. The
  preview's source is added to the page's code pointers for **Open the
  source** and handoffs.
- An unknown ID is reported as `<label>: unknown Workbench preview “<id>”`.
  What the lens then loads is [open question 1](#open-questions).

### Screenshots and handoff

Capture waits for the preview's readiness flag. It fails with the rendering
error, or with `Workbench preview did not finish rendering before capture`
after 8 seconds.

### Export

The [design-system export](export.md) plans one reference per preview state at
the page's viewports, loading the definition URL with `?state=`. After
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
- `preview/` is published to npm as `@canonic2/workbench` (`api.js`,
  `api.d.ts`) at the extension's version by each `workbench/v*` release, so
  `tsc` and editors resolve the import from an ordinary dev dependency. It
  must type-check with `Bundler` and `NodeNext` resolution, from ESM and
  CommonJS projects. Publishing uses npm trusted publishing from
  `extension.yml`; the release refuses a tag whose package version differs.

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
  file it read; config changes clear the memory cache. Authored builds also
  persist in the host's temporary directory across worker restarts, scoped to
  the project root, discovery options, compiler implementation, evaluated
  config and runtime version. Stale or unreadable artifacts rebuild from source;
  a disk-cache failure does not prevent compilation. Input-edit renders are not
  cached. Only discovered definitions inside the project compile. A request
  for one definition checks discovery rules directly without scanning every
  project file. Live source maps are separate files, fetched by developer tools.
- In an untrusted server (`isTrusted: false`), no definition runs, discovered
  files produce `Workbench previews require a trusted workspace.`, and preview
  routes answer 404. When trust applies is part of the
  [extension's trust contract](vscode-extension.md#workspace-trust).

### Runtime selection

The worker runs on the [bundled Electron runtime](capture.md#bundled-electron-runtime)
in Node mode when that runtime is used on the host, and otherwise on the
current process's executable (Node for the standalone server). `NODE_OPTIONS`
is removed from its environment. Compilation uses native `esbuild`. Each
platform VSIX bundles the matching pinned binary at build time; source
checkouts use the package's installed compiler. Projects install no compiler,
and the installed extension downloads none.

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
  placement (via `mergeCollections`); `workbench` lens state mapping; source
  closure hashing; authoring type checks; portable viewer and partial
  success; CLI `build` refusing nonempty output.
- [preview-runtime.test.js](../preview-runtime.test.js): renderer disposal,
  font and image readiness, hidden-frame readiness, lifecycle merging and
  cleanup, failed cleanups, concurrent host commands, async adapter errors.
- [preview-astro.test.js](../preview-astro.test.js): props, states, slots,
  assets, Node-only frontmatter, live input renders, portable states, missing
  dependencies and unsupported integrations, aliases and defines.
- [workbench/preview-host.test.js](../workbench/preview-host.test.js): warm
  host reuse, superseded loads, style and import-time body node retention,
  module isolation, recovery after failed mounts, listener and timer release,
  refusal of external pages. [workbench/preview.test.js](../workbench/preview.test.js)
  checks mounting into the warm spare frame.
- [preview-scripts.test.js](../preview-scripts.test.js) checks the shared
  compatibility injection, and that in-page fragments scroll in both switch
  positions while a mounted preview's `follow` sees links and forms only with
  actions on.
- [preview-runtime.test.js](../preview-runtime.test.js) "links open mapped
  previews…" and "without the actions switch…" cover matching, state-only
  targets, logged links and forms, `navigate` with actions off, release of
  `follow` on dispose, and the portable fallback. [preview.test.js](../preview.test.js)
  "preview links are validated…" covers validation, normalization, and the
  unknown-target problems.
- [requests.test.js](../requests.test.js) covers key matching, precedence,
  handler functions, GraphQL operations, logging, 404s for unmatched
  requests, Workbench bypass, and `pending`/`failed`/`delay`/`passthrough`.
  [preview-runtime.test.js](../preview-runtime.test.js) covers activation per
  render and `combine`; [preview-astro.test.js](../preview-astro.test.js)
  covers frontmatter fetches per state. [preview.test.js](../preview.test.js) "the project environment
  applies…" covers the single and per-adapter forms and a missing file.
- Not covered by automated tests: the XMLHttpRequest replacement and the
  project environment in a compiled preview (verified in Chrome on
  2026-10-03; see decisions). The recipes in
  [preview-data.md](../docs/preview-data.md) were run as written in Chrome on
  2026-10-03 (React, Vue, and Astro); none uses a third-party data or router
  library, since none is installed in this repository.
- Not covered by automated tests: the canvas and viewer handling of
  `navigate` (verified by hand; see the 2026-10-03 entries under decisions).
- Not covered: [manifest.test.js](../workbench/manifest.test.js) and
  [server.test.js](../server.test.js) have no TypeScript-preview cases; no test
  covers an unknown lens ID, manual-placement viewports, definition watching,
  an export that survives a failed portable build, CLI `check` or `init`
  beyond type generation, the worker's timeouts or restart, or packaging.

## Decisions and discoveries

- **Native compiler and persistent builds (2026-10-04).** An observed React
  page's initial descriptor request took 20.2 seconds in VS Code, followed by
  a 24 ms React mount. A source-checkout reproduction on macOS x64 measured
  21.4 seconds for WASM compilation and 2.2 seconds with native esbuild.
  Platform packaging supplies native binaries without runtime downloads.
  Separate source maps reduced that preview's served JavaScript from 19.5 MB
  to 5.1 MB. Cached artifacts avoid bundling again after a worker restart;
  source, config, compiler and runtime changes invalidate them. These are
  observations of one large project, not latency guarantees for every preview.
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
  page kept `implementations.built = { preview: "components/missing" }` with
  no `path`; a manual page placing a definition that declares
  `viewports: [mobile]` resolved and merged with all four viewports.
- **2026-10-03, manual CLI check**: an installed local build at
  `~/.vscode/extensions/canonic.canonic-workbench-0.6.0` (from an earlier
  working-tree state; four preview files differ from the current tree) ran
  `check` and `build` on an HTML-adapter fixture with Node 24 on macOS arm64;
  both succeeded and wrote the viewer. No other adapter, platform, or packaged
  VSIX was tried.

- **2026-10-03, website previews before `links`**: with the standalone server
  on `packages/website` and headless Chrome, every link in the Overview
  preview reached the browser with actions on: `/workbench/install/` loaded a
  route the server doesn't have, and the `.vsix` links downloaded from GitHub.
  With actions off, every link was stopped, including the page's own `#tour`
  and `#install` anchors. The hand-off in `actions.js` recognized only `.html`
  addresses, so no preview could reach another.
- **2026-10-03, after `links`**: on the same setup, with actions on, `#tour`
  scrolled the page (scrollY 1380), the `.vsix`, docs, and GitHub links were
  prevented and listed under **Actions** as `navigate`, and `/workbench/install/`
  switched the canvas to `previews/install.workbench.ts`. With actions off,
  the same links were prevented and nothing was logged. The resolved config
  had no problems. The portable viewer's `navigate` handling was not run.
- **2026-10-03, how two client projects mock page data in Storybook.** One
  renders real data-fetching pages and answers GraphQL operations by name from
  typed fixtures through a custom Apollo link, with per-story overrides for
  empty, never-resolving loading, and error states, a seeded session in
  `localStorage`, and a memory router that logs navigation. The other splits
  pages into a data provider and a view and renders the view inside its
  context provider with fixture values, seeding stores directly and swapping
  native modules at the bundler. `requests` covers the first without a custom
  client; environments and `aliases` already covered the second.
- **2026-10-03, request mocks in Chrome**: a React fixture whose page keeps
  `window.fetch` from load time and also uses XMLHttpRequest rendered its
  default, empty, loading, and 500 states from mocks; the project environment
  wrapped the preview's and its `setup` ran; a POST was listed under
  **Actions**.
- **The warm preview host carries a `<base>`** pointing at the compiled
  output, so a fragment-only `href` resolves to another document there; this
  is why `actions.js` scrolls fragments itself.

## Implementation gaps

- **Script navigation is not intercepted.** `location.assign`, `location.href`
  writes, `window.open`, and `form.submit()` (which fires no submit event)
  still leave the preview. Sources route them through `context.navigate`.
- **Request mocks cover fetch and asynchronous XMLHttpRequest only.**
  Synchronous XHR, WebSocket, EventSource, `navigator.sendBeacon`, and
  requests from service workers or workers reach the network. Batched GraphQL
  requests (an array body) are matched without an operation name.
- **Modifier clicks are claimed too.** A Cmd- or Ctrl-click on a link in a
  preview is mapped or logged like a plain click, never opened in a new tab.

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
   page without a `path`, and the canvas builds the URL as base plus
   `undefined`, so it appears to load `<root>/undefined`. The same happens with
   no problem at all when previews are disabled, untrusted, or none are
   discovered, because mapping is skipped. Should the lens be dropped, or show
   a named error?
2. **Manual-placement viewports.** The merge adopts a preview's viewports only
   when the authored page has none, but both config readers always fill all
   four, so the definition's `viewports` never apply to a manually placed
   page, on the canvas or in export. [Core](core.md#canvas-and-artboard) says
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
