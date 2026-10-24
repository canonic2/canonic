# Core workbench contract

This contract applies to every page: authored pages, discovered TypeScript
previews, and imported catalog entries. The implementation is in
[workbench.js](../ui/legacy/workbench.js), [page-list.js](../ui/legacy/page-list.js),
[manifest.js](../contracts/manifest.js), [config.js](../ui/legacy/config.js)
(browser), and the native [config reader](../server/src/config/read.rs) (server).
The Node [config reader](../server/node/config.js) is a parity-test oracle. The editor host, preview
compilation, and screenshot processes have separate
[extension](integrations/vscode-extension.md), [TypeScript preview](pages/previews.md), and
[capture](review/capture.md) contracts.

## Purpose

Workbench is for designing inside the codebase. Pages are designed and
reviewed with the project's own components, styles and data, at real device
widths, beside their implementations, rather than in a separate design tool
whose output is rebuilt in code. It renders in a browser: React Native
components render through React Native Web, which is less exact than a
device but keeps them on the same canvas as everything else.

## Terminology

Space, collection, group, page, scenario, artboard, iteration, lens, and the
parts of the interface (sidebar, top bar, canvas, toolbar, view controls) are
defined in [glossary](glossary.md), which code, guides, and specs follow. Pages
nest as Space › Collection › (Group) › Page › Scenario › Artboard › Iteration,
with an implicit scenario and artboard when none is declared
([the page hierarchy](glossary.md#the-page-hierarchy)); [artboards and iterations](pages/iterations.md)
owns the lower levels.

## Configuration and serving

- A project opts in with `workbench.yaml` at its workspace root. One loopback
  server serves the project at `/`, its bundled workbench at `/_workbench/`,
  and compiled TypeScript previews at their definition paths
  (`/<path>.workbench.ts`) with assets under `/_workbench/previews/`, all on
  one origin. Workbench does not copy files into the project.
- `workbench.local.yaml` is optional. It replaces top-level values except that
  `implementations` and space `sizes` merge by key, one level deep. A nested
  `start` block supplied locally replaces the whole committed `start` block,
  and a local `previews` value replaces the committed one.
- The config accepts block YAML. Names and IDs used in addresses are checked;
  invalid entries are reported rather than silently converted into pages.
  The browser uses `manifest.js`; the native server implements matching
  implementation, size, and `previews` rules, checked by config parity tests.
- `collections` contains the ordered collections. Each collection contains pages
  and optionally one level of groups (`group:`). `collections` may be omitted
  unless `previews: false` and no implementation has a catalog; then a config
  with no usable page fails with *nothing to show*.
- `previews` is a map (`include`, `config`, `icon`, `icons`, `lensLabel`) or `false`. Discovery is on by
  default; `false` turns it off and no preview code runs. An invalid value is
  a problem and leaves previews off until fixed. Discovery, definitions, and
  the compiler are specified in [TypeScript previews](pages/previews.md).
- Icon-only authored collections style matching imported collections and stay
  in the configuration editor. Empty collections are hidden after imports. Icon
  precedence and order-independent merging are specified in
  [preview placement](pages/previews.md#sidebar-placement).
- Discovered previews and imported catalog collections join authored
  collections with the same name. A discovered preview whose definition is
  also listed as an authored `src` stays one page: the authored label,
  lenses, and code pointers are kept, and the definition supplies its scenarios.
- A failed catalog or preview compilation leaves usable authored pages
  available and adds a problem. One invalid preview definition does not hide
  the others.
- `GET /_workbench/config` answers with the server-resolved configuration,
  imported collections (`catalogCollections`), absolute source paths, and
  `problems`. It is read fresh on every request, except docs pages' example
  listings, which it shows as last known and refreshes in the background
  ([docs pages](pages/docs-pages.md#problems)). The VS Code sidebar asks the
  server for imported collections because its webview cannot fetch them directly.

The [Configure pages contract](pages/configuration-editor.md) owns draft editing
and committed-file writes. [Access boundaries](system/access-boundaries.md)
distinguishes loopback binding, execution trust and file/request containment;
one does not imply the others. [Hosting](integrations/hosting.md) defines which
services exist in the editor, standalone browser and portable viewer.

## Problem reporting

- Every configuration, catalog, preview, and implementation problem names
  where it came from, for example
  `Pages › Auth › Sign in: scenario id “Error” must be kebab-case`.
- Every surface shows the same shared problems list: the VS Code
  sidebar above its page list, and the canvas wherever it runs (the editor tab,
  **Open Canvas in Browser**, and a standalone server). `/_workbench/config`
  lists them under `problems`.
- The list covers every entry dropped while reading the YAML (collections,
  groups, pages, scenarios, and invalid `src` values), implementation,
  size, mapping, code-pointer, `previews`, catalog, and
  preview-compilation problems, and implementations that are not running
  (except lens-scoped Gate availability below),
  including a stopped Storybook used only by mappings and a mapped Simulator
  device that is not booted. See
  [the extension contract](integrations/vscode-extension.md#sidebar-and-problems).
- Missing previews, invalid configuration, stopped implementations, and
  capture errors give a named problem. A failure in one imported catalog must
  not remove valid authored pages.
- Gate service availability belongs to the affected lens. A stopped,
  starting, failed or unavailable Gate service does not add a global sidebar
  warning or delay unrelated page listing. Opening its lens shows the named
  error on the canvas. Invalid Gate configuration remains a shared
  configuration problem; see [implementations](pages/implementations.md#url-implementations).

## Selection and navigation

- One selection is encoded as `#<src>[:<scenario>]@<size>[~<lens>]`. For example,
  `#pages/sign-in.html:error@mobile~staging` names a page, scenario, artboard size,
  and implementation. The default scenario and design lens are omitted; the
  workbench writes the resolved size key for an artboard and omits it in a
  docs lens. Docs addresses can also name `!<example>`. It normalizes the
  address after routing to the view it actually shows. Selected iterations
  follow the scenario as `+<iteration>` or `+<artboard>/<iteration>` parts
  ([address](pages/iterations.md#address)); `+` is reserved like `:`, `!`,
  `~` and `@`.
- The sidebar lists the collections, then the chosen collection's pages;
  search finds pages across collections, including scenario and group labels. A
  page with multiple scenarios expands to its scenarios. A single-scenario authored
  page does not expand.
- A lens choice persists across page picks. A page without that lens shows
  its design. An imported implementation-only page always uses its own lens;
  a discovered TypeScript preview uses its own authored lens. Every lens has
  a customizable display label independent of its identity and rendering
  kind; defaults and configuration are in the [lens contract](canvas/lenses.md).
- In VS Code, sidebar picks and canvas selections travel as `wb-go` and
  `wb-here`; the [extension contract](integrations/vscode-extension.md#editor-surfaces-and-messages)
  defines the relay and its ordering.

## Persistence

[Persistence and restoration](canvas/persistence.md) owns the durable state contract.
On opening an ordinary canvas, an explicit destination wins; otherwise restore
the last valid selection for that space, then fall back to its first page in
resolved order. Usable spaces must not open to an unselected blank canvas.
VS Code owns per-workspace/per-space records independently of server ports;
switching spaces preserves each selection. Restart restoration saves logical
view parameters and mounts fresh content. Live frame retention below preserves
runtime state only while those documents remain alive.

First-page startup fallback is implemented for a canvas without an addressed
page, including space switches. Complete-selection persistence remains required
implementation work; the existing URL and hidden-webview retention do not
establish restart persistence. The owning spec supplies validation, lifecycle,
storage-failure behavior, browser limits and acceptance.

## Canvas and artboard

The [interaction contract](canvas/interaction.md) owns camera, focus and input
routing; [annotations](canvas/annotations.md) owns drawing/editing and local
annotation lifetime. Screenshot/handoff acquisition remains in [capture](review/capture.md).

The [rendering-flow contract](system/rendering-flow.md) requires one Workbench shell and
content-only renderers. Canvas labels, grid, camera and global controls belong
to that shell; none is mounted inside an artboard. The backend provides project
data and compiled/rendered/proxied content, not Workbench application markup.
The [multiple-artboard system](canvas/multiple-artboards.md) owns instance state,
geometry and review/context. Scenarios remain alternatives; a scenario's declared
artboards are shown together as a group, each with at most one selected
iteration ([artboards and iterations](pages/iterations.md)). Outer control layout
stays unchanged by this refactor.

- Config and catalog discovery show **Loading pages…**. Sidebar selection updates
  immediately. Preview navigation follows the [200 ms grace period](system/rendering-flow.md#navigation-feedback):
  retain the outgoing view under its settled labels and context until the target
  is ready or the deadline expires, then swap or show **Loading preview…**.
  The first preview uses the same delayed loader without outgoing content.
  The shell owns application loading status. Loading indicators honor reduced motion.
- The size switcher uses the space's ordered sizes and each page's supported
  sizes, including page-specific sizes. With no space configuration the
  defaults are Fit, Laptop, Mobile and Resizable. Filled axes follow the
  available canvas; fixed dimensions remain fixed when the editor is smaller.
  The [sizes contract](canvas/sizes.md) owns the schema, menu, editing and defaults.

- A content renderer may prepare a replacement in a spare content iframe. It
  must not use a spare Workbench application document. Promotion waits for
  matching destination readiness; superseded loads cannot replace a newer selection
  or put outgoing pixels beneath the new destination's label.
- Retained content sessions, including previews and Docs, preserve their attached iframe
  across selections: one active session and up to three inactive
  sessions for 15 minutes after departure. Returning preserves form edits,
  scroll, menus, and application state. Inactive frames are inert, keep their
  layout dimensions, and cannot send navigation or keyboard commands to the
  active canvas. A loading replacement is protected until promoted or cancelled.
- Docs use this same session budget across lenses and content kinds. Exact
  retained returns show immediately and check revisions in the background;
  new destinations may reuse an idle same-lens Docs document under the
  [warm Docs pool contract](preview-pipeline/warm-runtime.md#warm-docs-pool).
- Oldest inactive sessions are evicted first; expiry, Reload, configuration
  refresh, and canvas closure release their resources. Managed hosts dispose
  adapters and tracked page work before removal. Returning to a managed preview
  checks its source revision and remounts if it changed. Sessions are local to
  the open canvas; no destination page is predicted or preloaded. The
  [preview pipeline](preview-pipeline/README.md) adds a warm runtime of shared
  code per project, not a predicted page, and keeps retained frames current
  through hot updates. The [warm-runtime status](preview-pipeline/warm-runtime.md#status)
  identifies the managed per-page return and memory-bound gaps.
- Managed previews wait for mounting, fonts, and visible images before becoming
  ready. Offscreen lazy images keep native lazy loading and do not delay the
  frame swap. Readiness must also progress when the browser suspends animation
  frames in the hidden loading iframe.
- The top bar exposes the page with its scenario or Storybook story picker, the
  lens switcher, the size switcher, reload, source files when resolved, a copyable
  reference, **Open on its own**, preview controls for a ready TypeScript
  preview, and **More** (page configuration and design-system export when the
  server is running). Narrow canvases fold secondary actions into **More**.
  The toolbar, with the annotation, screenshot, and handoff actions, floats
  over the canvas.
- Canvas zoom scales the artboard without changing its layout size; captures
  use the real artboard size. **Recenter view** in the view controls
  centers the artboard at its current zoom without reloading or scrolling the preview.
  A preview from an external origin remains inside an
  iframe; the workbench does not read its DOM directly.

## Verified behavior

- `previews` validation: `manifest.previews` returns `false` and adds a
  problem for a non-map, an empty or escaping `include`, or an absolute or
  escaping `config` ([manifest.js](../contracts/manifest.js)).
- Merging a discovered preview into an authored page is done by
  `manifest.mergeCollections`, used by both the sidebar and the canvas.
- Managed-document readiness uses a signal with a 50 ms compatibility poll.
  After 300 unsuccessful checks the destination fails rather than becoming
  visible unready; visible images are awaited for at most 3 seconds
  ([content-frames.js](../ui/legacy/content-frames.js), [browser.js](../ui/preview/browser.js)).

## Implementation gaps

- **Dropped authored entries are invisible in VS Code.** Problems for dropped
  collections, groups, pages, and scenarios go only to the canvas webview's
  console. The native [config reader](../server/src/config/read.rs) skips them, so the sidebar's
  problems list never shows them.
- **No problems list in a standalone browser.** The canvas does not display
  `problems` from `/_workbench/config`; in a browser, catalog and preview
  failures are visible only by reading that route.
- **No sidebar problems when nothing is imported.** With `previews: false` and
  no catalog, the VS Code sidebar builds from the YAML without asking the
  server, so implementation and mapping problems are not shown there.
- **Some failures never become problems.** A stopped Storybook used only by
  mappings, a mapped Simulator whose device is not booted, and an authored
  entry with an invalid `src` or scenario id fail only on the canvas or are
  dropped silently; see [implementations](pages/implementations.md#current-gaps) and
  [Storybook](pages/storybook.md#current-gaps).

## Verification points

- [address.test.js](../ui/legacy/address.test.js) checks address parsing,
  writing, and round trips.
- [page-list.test.js](../ui/legacy/page-list.test.js) checks listed scenarios and empty
  versus filtered lists.
- [config.test.js](../server/node/config.test.js) checks YAML reading, the local merge,
  and server-resolved paths and problems;
  [manifest.test.js](../contracts/manifest.test.js) checks the shared rules,
  including sizes.
- [preview.test.js](../ui/legacy/preview.test.js) checks spare-iframe
  replacement, superseded loads, warm managed-preview swaps, width modes, and
  story switching.
- [preview-host.test.js](../ui/legacy/preview-host.test.js) and
  [preview-runtime.test.cjs](../ui/preview/preview-runtime.test.cjs) check host reuse,
  disposal, hidden-iframe readiness, and lazy images.
- [preview.test.js](../server/node/preview.test.js) checks preview serving at definition
  paths, `previews: false`, and merging a discovered preview into an authored
  page.
- [server.test.js](../server/node/server.test.js) checks the config route and injection of
  `preview-compat.js` into served HTML.
- Not covered: no test checks that the sidebar and canvas show the same
  problems list.
