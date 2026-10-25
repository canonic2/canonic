# TypeScript Workbench previews

A Workbench preview renders a component or page from a project's own source,
in named scenarios, from a `.workbench.ts` or `.workbench.tsx` definition. The
user guide is [Workbench previews](../../docs/workbench-previews.md); this spec
records the contract. See
[compiler.cjs](../../server/node/preview/compiler.cjs), [compiler.rs](../../server/src/compiler.rs),
[evaluator.cjs](../../server/node/preview/evaluator.cjs),
[browser.js](../../ui/preview/browser.js), [catalog.rs](../../server/src/catalog.rs), and
[manifest.js](../../contracts/manifest.js).

**Status:** discovery, sidebar placement, scenarios, preview controls, actions,
links between previews, request mocks, project environments, the `workbench`
lens, capture, export, the portable build and the CLI are implemented for every
built-in and custom adapter. Unmet requirements (script navigation, request
types beyond fetch and asynchronous XHR, modifier clicks, definition-change
notices in the standalone browser canvas) are under
[implementation gaps](#implementation-gaps); unresolved choices are under
[open questions](#open-questions).

How previews are compiled, served, kept warm, made ready and updated after
edits is specified in the [preview pipeline](../preview-pipeline/README.md).

## Purpose and scope

Previews render project components on the canvas through Workbench's own
compiler, and the design-system export carries them as runnable, server-free
output. They cover HTML, React, Vue, Astro, React Native Web, and
project-registered adapters. They do not run a project's application server,
`astro.config.*`, or middleware; a whole application stays a
[URL lens](implementations.md#url-implementations).

- **Requirement:** a preview is a mock of its page, in
  the way a Storybook story is, not a running copy of the application.
  Workbench owns everything that would navigate away from it: links and forms
  open other previews through the canvas or are recorded as actions, and the
  Actions switch governs both. Nothing in a preview navigates the frame or
  starts a download. See [links and navigation](#links-and-navigation).
- Previews are discovered by default and open in their own authored lens,
  labelled **Preview** by default and customizable under the
  [lens contract](../canvas/lenses.md).
- Source compiles in the Workbench server's compiler, with project code in
  the Node evaluator ([server processes](#server-processes-lifecycle-and-trust)).
  Live and portable output share one
  adapter and render lifecycle, and that lifecycle is shared by every built-in
  and custom adapter.
- Managed previews use
  [retained iframe sessions](../preview-pipeline/warm-runtime.md#retained-sessions)
  and become ready as the [readiness contract](../preview-pipeline/readiness.md)
  defines.
- The preview mount fills the artboard width, including when project styles
  make the body a flex container. Loading, empty, and error content use the
  same width. The mount retains a viewport minimum height; web content does
  not need to stretch vertically.
- The export adds a `browser/` package that opens from a static HTTP server
  without Electron or Workbench; failed builds become warnings while successful
  previews remain; browser build products do not enter source hashes. The CLI
  `build` uses the same builder without a server.

## Workflows

All adapters mount project content through the [content-only renderer contract](../system/rendering-flow.md).
Project compilation, evaluation and server rendering remain backend responsibilities.
The resulting preview must not initialize Workbench's application shell inside
an artboard, even while loading or displaying errors.

### Author and discover

- A definition default-exports `definePreview({...})` from
  `@canonic2/workbench`. Workbench resolves that specifier to its own bundled
  module, even when the project installs the package, so builds never depend
  on the installed version. The framework itself (React, Vue,
  Astro, React Native Web) must come from the project.
- The [page index](../preview-pipeline/page-index.md) owns discovery patterns,
  ordering, skipped paths, saved-state reuse, evaluation and fallback. Discovery
  is enabled by default; `previews.include` replaces its patterns and
  `previews: false` disables project preview code and closes the compilers.
  Native listing is independent of the compilers and the Node evaluator. Untrusted
  workspaces list no definitions.

- A definition that fails to load or validate becomes one problem prefixed
  with its file; other previews still load. A second definition with an
  existing ID fails with `Duplicate preview id`.

### Sidebar placement

- Each preview becomes a page whose `src` is the definition file. The title's
  first segment names the collection, the last the page, and the segments
  between are joined with ` / ` into one group. A title without `/` goes in
  **Previews**; a missing title uses the ID. Page and collection icons follow
  the [icon rules](page-tree.md#icons).
- Imported previews merge into the authored tree, including placeholder
  collections, same-name groups and authored pages whose `src` is a definition
  file, under the page tree's [merging rules](page-tree.md#merging).
  Authored-page size precedence follows the
  [sizes contract](../canvas/sizes.md#typescript-previews).
- **Requirement:** the sidebar and canvas update when a
  definition file matching discovery is added, removed, renamed, or changes
  its title, scenarios, or sizes, without **Workbench: Refresh Pages**; see
  [the extension's refresh contract](../integrations/vscode-extension.md#refresh).
  In VS Code, the [server](../system/server.md#the-resident-index-and-watcher) watches
  the space and the extension runs the same rebuild as a `workbench.yaml`
  edit when the listed definitions change.

### Open, scenarios, controls, and actions

For the [multiple-artboard support system](../canvas/multiple-artboards.md), each instance
owns its renderer, requested/settled view, controls, action log, and pending-load
generation. Two copies of the same preview must coexist without sharing a
mutable iframe; content navigation updates its originating instance. Shared
origin storage/backend state remains external state. The
[code plan](../canvas/artboard-system.md) reuses compiler and framework mount
contracts through explicit runtime adapters. The renderer behavior below
applies independently within each isolated artboard.

- A discovered page's design lens is the preview itself. Its customizable
  label defaults to **Preview**; `previews.lensLabel` sets the discovery
  fallback and an explicit page's `lensLabel` overrides it under the
  [lens contract](../canvas/lenses.md). Its address is the definition path; `?scenario=<id>` selects a
  scenario and the first scenario is the default. A direct preview URL naming
  an unknown scenario renders an error; through the canvas, an unknown
  scenario in the address falls back to the first scenario and the address is
  normalized. Scenario rules shared with other page kinds are in
  [scenarios](scenarios.md).
- The runtime marks the rendered scenario and iteration on the document element
  as `data-wb-scenario` and, when an iteration renders, `data-wb-iteration`.
- Same-origin `.workbench.ts(x)` addresses load into a warm preview host
  rather than navigating the iframe. A newer load supersedes a queued one, and
  page-level listeners, timers, and animation frames remain owned by that
  session until it is evicted or reset. Retention, its budget and the
  return revision check are [retained sessions](../preview-pipeline/warm-runtime.md#retained-sessions).
  The warm-document path can reuse one session across pages; its per-page
  return-state limitation is recorded in [warm runtime](../preview-pipeline/warm-runtime.md#gaps).
- Body nodes created during a preview module's import, such as SVG symbol
  sprites, are retained by compiled module URL and restored before its next
  mount. They are detached while another module is active or the host is reset.
  Nodes created during mounting, including portals, are cleared on reset or eviction.
  Imports that finish after being superseded still retain their nodes for a
  later return; a failed mount does not discard import-time nodes.
- When ready, the preview reports its inputs, controls, docs, and last 30
  actions to the canvas, which shows **Preview controls**: one field per
  control (`text`, `number`, `boolean`, `select`, `json`), **Reset scenario**,
  **Actions**, and **Documentation**. Edits re-render the current scenario with
  input overrides and last until reset, reload, source revision change, or session eviction;
  they never write files or affect references or exports.
- Saving the definition or any file in its dependency graph updates the open
  preview without a manual refresh: edits apply as hot updates pushed from the
  server, and edits that cannot apply in place reload the preview
  ([hot updates](../preview-pipeline/hot-updates.md)).
- Astro input edits re-render through the server; frontmatter runs in the
  Node evaluator.

### Links and navigation

- `links` maps addresses to targets: a preview ID, `{ preview, scenario }`, or
  `{ scenario }` for the same preview. Validation rejects a non-map and targets
  that are neither; the catalog carries each target normalized to
  `{ preview, scenario? }`. A target naming an unknown preview or scenario is a
  problem (`<file>: link <href> names unknown Workbench preview “<id>”.`, or
  `… names unknown scenario “<scenario>” of <id>.`); the other previews still load.
- While a preview is mounted, the runtime ([browser.js](../../ui/preview/browser.js))
  registers `wbPreviewActions.follow`, which [actions.js](../../ui/legacy/actions.js)
  calls for every link click and form submit with actions on, after in-page
  fragments and before its `.html` hand-off. A key matches when the link and
  the key, both resolved against `document.baseURI`, have the same origin,
  path, and query; the fragment is ignored.
- A match posts `{ type: 'workbench-preview', event: 'navigate', id, preview,
  scenario }` to the parent. The canvas finds the page whose resolved preview
  has that ID, reveals it, and routes to it with the scenario (unknown scenarios fall
  back to the default); an ID with no page is ignored. The portable viewer
  selects the preview from its catalog.
- No match: the click or submit is prevented and logged through the context's
  `action`: `navigate` with the raw `href`, or `submit` with the raw `action`
  (omitted when empty) and the form's fields as an object (files by name).
- `context.navigate(to)` takes the same targets, does nothing with actions
  off, and without a parent frame logs `navigate` with `<id>[?scenario=<scenario>]`.
- A portable page has no `actions.js`; the runtime installs its own capture
  listeners and behaves as actions on, leaving fragment links to the browser.
- With actions off, `actions.js` stops links and forms before the runtime sees
  them, and nothing is logged.

### Data: request mocks and the project environment

- **Requirement:** a preview supplies the data its real
  page loads, in whatever way that page loads it, for every adapter:
  `inputs` for props, an environment for providers and stores, `requests` for
  the page's own network calls, and `aliases` for modules. Projects can run
  any code in environments and hooks; `requests` is the built-in tool for the
  common case.
- `requests` (preview, scenario and iteration) maps keys `[METHOD] /path [Operation]` or a
  full URL to a response object or a function `(request, context)`.
  Validation checks key shape and value type. Matching
  ([requests.js](../../ui/preview/requests.js)): the maps in the
  [merge order](iterations.md#what-an-iteration-changes), declaration order
  within each; a bare path matches any origin;
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
- Astro: the compiler swaps `globalThis.fetch` for the rendered iteration's,
  scenario's and preview's maps, in the
  [merge order](iterations.md#what-an-iteration-changes), around
  each server render, relative URLs resolving against `http://localhost/`,
  with a 10-second abort. Unmatched requests are not logged there.
- `workbench.config.ts` `environment` (a path from the project root, inside
  the project, or a map from adapter name to such a path, so React and Vue
  previews get environments of their own; an unlisted adapter gets none) is imported into every compiled preview and composed with the
  preview's by `combine` in [browser.js](../../ui/preview/browser.js): project
  `setup`/`mount` first with cleanups last, `ready` and `configure` in the
  same order, and `wrap` outermost.

### Compare a design via a `workbench` lens

- An implementation with `kind: workbench` has no `base` or `url`; `start`,
  `catalog`, and a `root` other than `.` are reported. A page maps it to a preview
  ID. The server maps each preview scenario ID to `/<file>?scenario=<id>`; the
  lens loads the definition path, a design scenario loads the preview
  scenario with the same ID, and the design's first scenario and any scenario
  without a match show the preview's first scenario. The
  preview's source is added to the page's code pointers for **Open the
  source** and handoffs.
- An unknown ID is reported as `<label>: unknown Workbench preview “<id>”`.
  What the lens then loads is [open question 1](#open-questions).

### Screenshots and handoff

Capture waits for the preview's readiness flag. It fails with the rendering
error, or with `Workbench preview did not finish rendering before capture`
after 8 seconds.

### Export

The [design-system export](../review/export.md) plans one reference per preview scenario at
the page's sizes, loading the definition URL with `?scenario=`. After
references, it asks the compiler for the portable build and adds the `browser/`
package and catalog. A preview that fails to build is a warning. **Requirement:** a portable build that fails entirely, including a
compiler or evaluator failure, is also a warning, and the export still delivers its sources
and references without `browser/`; see [export](../review/export.md#job-lifecycle).
The source closure is
the compiler's resolved local module graph (outside `node_modules`); packages
reached through `node_modules` are listed by name and version.

### Portable build and CLI

- The portable build writes each preview's production bundle, an interactive
  viewer (`index.html`, `viewer.js`, `viewer.css`), the canvas's own
  `preview-controls.js`/`.css`, and `workbench.json` (version 1, name,
  previews, warnings). The viewer offers search, Scenario and Size menus,
  resizable dimensions, Reload, the shared controls, and **Open preview**;
  its address keeps the selection (`size` is a resolved size key, with `width`
  and `height` when that size is resizable). The menu uses the selected page's
  resolved sizes, including configured and page-owned keys.
- Astro previews carry every authored scenario pre-rendered. Their controls are
  removed and a note is appended to their docs, since input edits need a live
  render.
- `workbench-server` ships in the extension with `init`, `list`, `check` and
  `build` commands ([build.rs](../../server/src/build.rs)). `init` writes
  `workbench-env.d.ts` and a minimal `workbench.yaml` without overwriting.
  `list` prints the page index's answer. `check` evaluates and compiles every
  preview and exits nonzero on any failure. `build` refuses a nonempty output
  folder, writes successful previews, and exits nonzero when any failed.
  `check` and `build` honor `previews: false`. The commands run definitions
  on `--node` or the `node` on the `PATH` with no trust check; running them
  is the user's explicit choice.

## System behavior

### Definition contract

- `id`: kebab-case segments separated by `/`, unique per project. `adapter`:
  a kebab-case name. `source.entry`, and any scenario's or iteration's
  `source.entry`, must exist inside the project after resolving symbolic links.
- `scenarios`: a map of IDs following the shared ID rule
  ([scenarios](scenarios.md#declaring-scenarios)) to objects; absent or empty means one
  `default` scenario. A scenario may contain `label`, `source`, `inputs`,
  `fixtures`, `globals`, `requests`, `setup`, `play`, `ready`, and either
  `artboards` or `iterations`; the SDK types make the last two exclusive and
  validation rejects a scenario with both. Labels default to the ID in sentence
  case.
- A scenario's `artboards` and the `iterations` of its declared or implicit
  artboard follow
  [artboards and iterations](iterations.md#declaring-artboards-and-iterations),
  which owns their validation (ID syntax, artboard fields and sizes, iteration
  fields). A definition has no top-level `artboards` or `iterations`.
- `inputs`, `fixtures` and `globals` merge in the
  [iteration merge order](iterations.md#what-an-iteration-changes). All are
  structured-cloned per render. Validation checks that scenario, artboard and iteration `inputs`,
  `fixtures` and `globals` serialize as JSON, that `requests` keys and values
  have the request-mock shape, and that iteration hooks are functions.
- `sizes`: a nonempty list of kebab-case `SizeKey` values, including keys
  declared by the space. The [sizes contract](../canvas/sizes.md#typescript-previews)
  owns resolution, unknown-key reporting and authored-page precedence.
  `controls`: known types; `select` needs nonempty `options`.
- `styles`, `assets`, and `environment` resolve from the definition. Declared
  assets must stay inside the project.
- [api.d.ts](../../sdk/api.d.ts) types scenario inputs against preview inputs.
- `sdk/` is published to npm as `@canonic2/workbench` (`api.js`,
  `api.d.ts`), so `tsc` and editors resolve the import from an ordinary dev
  dependency. It must type-check with `Bundler` and `NodeNext` resolution,
  from ESM and CommonJS projects.

### Adapters and project configuration

- Built-ins: `html`, `react`, `vue`, `astro`, and `react-native-web`. Each
  [adapter](../adapters/README.md) has its own spec: its source, build,
  project code on the server, browser mount and edit behavior. Project styles
  can override an adapter's host layout; custom runtimes own their host styling.
- `workbench.config.ts`, or `previews.config`, is evaluated like a definition.
  Its `adapters` register [custom adapters](../adapters/custom.md). It also sets
  global `rules`, `plugins`, `aliases`, `dedupe`, `resolveExtensions`, and
  `define`. Rules and plugins are Rspack rules and plugins, applied after
  Workbench's own ([plugins](../preview-pipeline/plugins.md#project-plugins)).
- Custom adapters, compiler plugins, live output, and portable output share the
  same entry and [browser runtime](../../ui/preview/browser.js).

### Server processes, lifecycle and trust

Compilation runs in `workbench-server`'s
[preview pipeline](../preview-pipeline/README.md), which owns how the
compilers are configured, build and pre-warm
([compiler](../preview-pipeline/compiler.md)). Evaluation and Astro rendering
run in its Node evaluator; the server's
[processes and connections](../system/server.md#processes-and-connections) own
the evaluators' processes, lifetimes, deadlines, runtime selection and request
timing.

- In an untrusted server (`isTrusted: false`), no definition runs, discovered
  files produce `Workbench previews require a trusted workspace.`, and preview
  routes answer 404. When trust applies is part of the
  [extension's trust contract](../integrations/vscode-extension.md#workspace-trust).

### Readiness and errors

- A render aborts the previous signal and runs cleanups in reverse; a failed
  cleanup does not stop the others. `setup`, `play` and `ready` run in the
  [iteration order](iterations.md#what-an-iteration-changes), with the source
  mounting after `setup`; readiness then waits as the
  [readiness contract](../preview-pipeline/readiness.md) defines.
- Thrown errors, unhandled rejections, adapter errors, and `context.error`
  clear readiness, release resources, and show the error in the frame.
  **Reset scenario** recovers.
- Problems from discovery, validation, compiler configuration, and lens mapping join
  the config's `problems`; see [core](../core.md#problem-reporting).

## Acceptance criteria and verification points

- [preview.test.js](../../server/node/preview.test.js): discovery, ID and scenario validation, and
  isolation of invalid definitions; HTML scripts, inline modules, CSS,
  `srcset`, and assets; React and Vue SFCs with project framework versions;
  custom adapters and plugins on the shared path; project escape and excluded
  definitions; server catalog, compiled pages, runnable export, and closing
  previews; untrusted and disabled workspaces never executing definitions; manual
  placement (via `mergeCollections`); `workbench` lens scenario mapping; source
  closure hashing; authoring type checks; portable viewer and partial
  success; CLI `build` refusing nonempty output.
- [preview-runtime.test.cjs](../../ui/preview/preview-runtime.test.cjs): renderer disposal,
  font and image readiness, hidden-frame readiness, lifecycle merging and
  cleanup, failed cleanups, concurrent host commands, async adapter errors.
- [preview-astro.test.js](../../server/node/preview-astro.test.js): props, scenarios, slots,
  assets, Node-only frontmatter, live input renders, portable scenarios, missing
  dependencies and unsupported integrations, aliases and defines.
- [legacy/preview-host.test.js](../../ui/legacy/preview-host.test.js): warm
  host reuse, superseded loads, style and import-time body node retention,
  module isolation, recovery after failed mounts, listener and timer release,
  refusal of external pages. [legacy/preview.test.js](../../ui/legacy/preview.test.js)
  checks mounting into the warm spare frame.
- [files.rs](../../server/src/serve/files.rs) tests check the shared
  compatibility injection.
- [preview-runtime.test.cjs](../../ui/preview/preview-runtime.test.cjs) "links open mapped
  previews…" and "without the actions switch…" cover matching, scenario-only
  targets, logged links and forms, `navigate` with actions off, release of
  `follow` on dispose, and the portable fallback. [preview.test.js](../../server/node/preview.test.js)
  "preview links are validated…" covers validation, normalization, and the
  unknown-target problems.
- [requests.test.cjs](../../ui/preview/requests.test.cjs) covers key matching, precedence,
  handler functions, GraphQL operations, logging, 404s for unmatched
  requests, Workbench bypass, and `pending`/`failed`/`delay`/`passthrough`.
  [preview-runtime.test.cjs](../../ui/preview/preview-runtime.test.cjs) covers activation per
  render and `combine`; [preview-astro.test.js](../../server/node/preview-astro.test.js)
  covers frontmatter fetches per scenario. [preview.test.js](../../server/node/preview.test.js) "the project environment
  applies…" covers the single and per-adapter forms and a missing file.
- Dedicated verification gaps include XMLHttpRequest replacement in a compiled
  preview, project environments with third-party clients/routers, and canvas
  and portable-viewer navigation. Browser recipes are in
  [preview data](../../docs/preview-data.md).
- Definition watching through the extension's rebuild, evaluator timeout/restart paths, CLI `check`/`init` beyond
  type generation, and installed cross-platform CLI use need dedicated
  acceptance coverage. See the [page-index evidence](../preview-pipeline/page-index.md#acceptance-and-evidence)
  for native listing and packaging evidence.

## Design constraints

- Project code runs only in the server's
  [evaluators](../system/server.md#processes-and-connections). Astro frontmatter
  runs on the server side; only rendered HTML and client assets reach browsers.
- The [readiness contract](../preview-pipeline/readiness.md) owns rendering,
  font and image waits, including progress in hidden frames.
- Asset requests recheck configuration and trust without rebuilding the catalog.
- Fragment navigation is handled inside the preview because the warm host's
  `<base>` points at compiler output. A fragment must scroll its source page
  rather than resolve against that output directory.
- Request mocks provide data-fetching states; environments provide application
  context; aliases provide platform substitutions. None requires running a
  project's application server.

## Implementation gaps

- **The standalone browser canvas misses definition changes.** It gets no
  notice when listed definitions change and needs a reload.
- **Script navigation is not intercepted.** `location.assign`, `location.href`
  writes, `window.open`, and `form.submit()` (which fires no submit event)
  still leave the preview. Sources route them through `context.navigate`.
- **Request mocks cover fetch and asynchronous XMLHttpRequest only.**
  Synchronous XHR, WebSocket, EventSource, `navigator.sendBeacon`, and
  requests from service workers or workers reach the network. Batched GraphQL
  requests (an array body) are matched without an operation name.
- **Modifier clicks are claimed too.** A Cmd- or Ctrl-click on a link in a
  preview is mapped or logged like a plain click, never opened in a new tab.

## Open questions

- **Unavailable Workbench lens.** An unknown preview ID, disabled previews or
  unavailable discovery must yield an explicit unavailable lens. Decide
  whether to hide that lens or render its named error; never navigate to an
  unresolved path.
- **Scenario data.** Validation uses JSON serialization while runtime isolation
  uses `structuredClone`. Decide whether JSON data or the broader cloneable
  set is the authoring contract.
- **Trust changes.** Live workspace-trust transitions and added spaces are
  governed by the [extension contract](../integrations/vscode-extension.md#open-questions).
