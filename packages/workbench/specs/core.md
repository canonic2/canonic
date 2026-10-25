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

**Status:** implemented: configuration and serving, the shared problems list,
addresses and lens selection. Not implemented: reporting a stopped
mapping-only Storybook or an unbooted mapped Simulator as problems; see
[implementation gaps](#implementation-gaps).

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
([the page hierarchy](glossary.md#the-page-hierarchy)). The
[page tree](pages/page-tree.md) owns spaces, collections, groups and pages;
[scenarios](pages/scenarios.md) owns scenarios; [artboards and iterations](pages/iterations.md)
owns the levels below.

## Configuration and serving

- A project opts in with `workbench.yaml` at its workspace root. One loopback
  server serves the project at `/`, its bundled workbench at `/_workbench/`,
  and compiled TypeScript previews at their definition paths
  (`/<path>.workbench.ts`) with assets under `/_workbench/previews/`, all on
  one origin. Workbench does not copy files into the project.
- `workbench.local.yaml` is optional and merges over the committed file under
  the [page tree's local merge rules](pages/page-tree.md#local-overrides).
- The config accepts block YAML. Names and IDs used in addresses are checked;
  invalid entries are reported rather than silently converted into pages.
  The browser uses `manifest.js`; the native server implements matching
  implementation, size, and `previews` rules, checked by config parity tests.
- `collections` contains the ordered collections. Each collection contains pages
  and optionally one level of groups (`group:`). `collections` may be omitted
  unless `previews: false` and no implementation has a catalog; then a config
  with no usable page fails with *nothing to show*. Entry rules, imports,
  merging and order are in the [page tree](pages/page-tree.md).
- `previews` is a map (`include`, `config`, `icon`, `icons`, `lensLabel`) or `false`. Discovery is on by
  default; `false` turns it off and no preview code runs. An invalid value is
  a problem and leaves previews off until fixed. Discovery, definitions, and
  the compiler are specified in [TypeScript previews](pages/previews.md).
- Icon-only authored collections style matching imported collections and stay
  in the configuration editor. Empty collections are hidden after imports. Icon
  precedence and order-independent merging are specified in
  [page tree icons](pages/page-tree.md#icons).
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
  `Pages › Auth › Sign in: implementation “dev” maps scenario “locked”, which this page doesn’t declare.`
- Every sidebar shows the same shared problems list above its page list: the
  VS Code sidebar, and the sidebar at the left edge of a browser window
  (**Open Canvas in Browser** and a standalone server). The canvas in a VS Code
  editor tab has no sidebar of its own; the VS Code sidebar beside it shows the
  list. `/_workbench/config` lists them under `problems`, and a browser window
  without a server shows its own reader's problems.
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

- One selection is encoded as
  `#<src>[:<scenario>][+<iteration> | +<artboard>/<iteration> …][!<example>][@<size>][~<lens>]`.
  For example, `#pages/sign-in.html:error@mobile~staging` names a page,
  scenario, artboard size, and implementation. The first scenario and the
  design lens are omitted; the workbench writes the resolved size key for an
  artboard and omits it in a docs lens, which can name `!<example>` instead.
  Selected iterations follow the scenario
  ([address](pages/iterations.md#address)). It normalizes the address after
  routing to the view it actually shows. `:`, `+`, `!` and `~` are reserved
  in page paths, so the parts split unambiguously; the page tree reports an
  [authored `src`](pages/page-tree.md#collections-groups-and-pages-in-workbenchyaml)
  or [discovered path](pages/page-tree.md#imported-pages) that contains one.
- The sidebar lists the collections, then the chosen collection's pages;
  search finds pages across collections, including scenario and group labels.
  A page lists its scenarios under it only when it has two or more
  ([page list](pages/page-tree.md#the-page-list)).
- A lens choice persists across page picks. A page without that lens shows
  its default lens: its first docs lens, else its design. An imported implementation-only page always uses its own lens;
  a discovered TypeScript preview uses its own authored lens. Every lens has
  a customizable display label independent of its identity and rendering
  kind; defaults and configuration are in the [lens contract](canvas/lenses.md).
- In VS Code, sidebar picks and canvas selections travel as `wb-go` and
  `wb-here`; the [extension contract](integrations/vscode-extension.md#editor-surfaces-and-messages)
  defines the relay and its ordering.

## Persistence

An explicit destination wins over the last saved selection, which wins over the
space's first page; [persistence and restoration](canvas/persistence.md) owns that
precedence and what is saved.

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
  immediately; preview navigation, its 200 ms grace period and loading feedback
  follow [navigation](canvas/navigation.md).
- The size switcher uses the space's ordered sizes and each page's supported
  sizes, including page-specific sizes. With no space configuration the
  defaults are Fit, Laptop, Mobile and Resizable. Filled axes follow the
  available canvas; fixed dimensions remain fixed when the editor is smaller.
  The [sizes contract](canvas/sizes.md) owns the schema, menu, editing and defaults.

- A content renderer may prepare a replacement in a spare content iframe, under
  [navigation](canvas/navigation.md#navigation-feedback).
- Retained content sessions, including previews and Docs, preserve their attached
  iframe across selections within the budget, eviction and cleanup rules of
  [retained sessions](preview-pipeline/warm-runtime.md#retained-sessions).
- What a managed preview waits for before becoming ready, and its constants,
  are owned by [readiness](preview-pipeline/readiness.md).
- The top bar's controls are named in the [glossary](glossary.md#top-bar).
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
- Managed-document readiness polling and its limits are verified under
  [readiness](preview-pipeline/readiness.md).

## Implementation gaps

- **Some failures never become problems.** A stopped Storybook at a configured
  `url` used only by mappings, and a mapped Simulator whose device is not
  booted (without `catalog: true`), fail only on the canvas; see [implementations](pages/implementations.md#current-gaps) and
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
