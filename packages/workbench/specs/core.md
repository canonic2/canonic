# Core workbench contract

This contract applies to every page: authored pages, discovered TypeScript
previews, and imported catalog entries. The implementation is in
[workbench.js](../ui/legacy/workbench.js), [page-list.js](../ui/legacy/page-list.js),
[manifest.js](../contracts/manifest.js), [config.js](../ui/legacy/config.js)
(browser), and [config.js](../server/node/config.js) (server). The editor host, preview
compilation, and screenshot processes have separate
[extension](vscode-extension.md), [TypeScript preview](previews.md), and
[capture](capture.md) contracts.

## Purpose

Workbench is for designing inside the codebase. Pages are designed and
reviewed with the project's own components, styles and data, at real device
widths, beside their implementations, rather than in a separate design tool
whose output is rebuilt in code. It renders in a browser: React Native
components render through React Native Web, which is less exact than a
device but keeps them on the same canvas as everything else.

## Terminology

Space, collection, group, page, state, lens, and the parts of the interface
(sidebar, top bar, canvas, artboard, toolbar, view controls) are defined in
[glossary](glossary.md), which code, guides, and specs follow.

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
  The browser and server use the same implementation, size, and `previews`
  rules from `manifest.js`.
- `collections` contains the ordered collections. Each collection contains pages
  and optionally one level of groups (`group:`). `collections` may be omitted
  unless `previews: false` and no implementation has a catalog; then a config
  with no usable page fails with *nothing to show*.
- `previews` is a map (`include`, `config`, `icon`, `icons`, `lensLabel`) or `false`. Discovery is on by
  default; `false` turns it off and no preview code runs. An invalid value is
  a problem and leaves previews off until fixed. Discovery, definitions, and
  the worker are specified in [TypeScript previews](previews.md).
- Icon-only authored collections style matching imported collections and stay
  in the configuration editor. Empty collections are hidden after imports. Icon
  precedence and order-independent merging are specified in
  [preview placement](previews.md#sidebar-placement).
- Discovered previews and imported catalog collections join authored
  collections with the same name. A discovered preview whose definition is
  also listed as an authored `src` stays one page: the authored label,
  lenses, and code pointers are kept, and the definition supplies its states.
- A failed catalog or preview compilation leaves usable authored pages
  available and adds a problem. One invalid preview definition does not hide
  the others.
- `GET /_workbench/config` answers with the server-resolved configuration,
  imported collections (`catalogCollections`), absolute source paths, and
  `problems`. It is read fresh on every request, except docs pages' example
  listings, which it shows as last known and refreshes in the background
  ([docs pages](docs-pages.md#problems)). The VS Code sidebar asks the
  server for imported collections because its webview cannot fetch them directly.

## Problem reporting

- Every configuration, catalog, preview, and implementation problem names
  where it came from, for example
  `Pages › Auth › Sign in: state id “Error” must be kebab-case`.
- Every surface shows the same problems list: the VS Code
  sidebar above its page list, and the canvas wherever it runs (the editor tab,
  **Open Canvas in Browser**, and a standalone server). `/_workbench/config`
  lists them under `problems`.
- The list covers every entry dropped while reading the YAML (collections,
  groups, pages, states, and invalid `src` values), implementation,
  size, mapping, code-pointer, `previews`, catalog, and
  preview-compilation problems, and implementations that are not running,
  including a stopped Storybook used only by mappings and a mapped Simulator
  device that is not booted. See
  [the extension contract](vscode-extension.md#sidebar-and-problems).
- Missing previews, invalid configuration, stopped implementations, and
  capture errors give a named problem. A failure in one imported catalog must
  not remove valid authored pages.

## Selection and navigation

- One selection is encoded as `#<src>[:<state>]@<size>[~<lens>]`. For example,
  `#pages/sign-in.html:error@mobile~staging` names a page, state, artboard size,
  and implementation. The default state and design lens are omitted; the
  workbench writes the resolved size key for an artboard and omits it in a
  docs lens. Docs addresses can also name `!<example>`. It normalizes the
  address after routing to the view it actually shows.
- The sidebar lists the collections, then the chosen collection's pages;
  search finds pages across collections, including state and group labels. A
  page with multiple states expands to its states. A single-state authored
  page does not expand.
- A lens choice persists across page picks. A page without that lens shows
  its design. An imported implementation-only page always uses its own lens;
  a discovered TypeScript preview uses its own authored lens. Every lens has
  a customizable display label independent of its identity and rendering
  kind; defaults and configuration are in the [lens contract](lenses.md).
- In VS Code, sidebar picks and canvas selections travel as `wb-go` and
  `wb-here`; the [extension contract](vscode-extension.md#editor-surfaces-and-messages)
  defines the relay and its ordering.

## Canvas and artboard

The [rendering-flow contract](rendering-flow.md) requires one Workbench shell and
content-only renderers. Canvas labels, grid, camera and global controls belong
to that shell; none is mounted inside an artboard. The backend provides project
data and compiled/rendered/proxied content, not Workbench application markup.
The [multiple-artboard system](multiple-artboards.md) owns instance state,
geometry and review/context. Ordinary states remain alternatives; only explicit
page artboards create a group. Outer control layout stays unchanged by this refactor.

- Config and catalog discovery show **Loading pages…**. The first iframe
  preview shows **Loading preview…** until the target content is ready. A later
  transition may retain outgoing content only under its actual settled identity;
  it is concealed before new destination labels are displayed. The shell owns
  application loading status. Loading indicators honor reduced motion.
- The size switcher uses the space's ordered sizes and each page's supported
  sizes, including page-specific sizes. With no space configuration the
  defaults are Fit, Laptop, Mobile and Resizable. Filled axes follow the
  available canvas; fixed dimensions remain fixed when the editor is smaller.
  The [sizes contract](sizes.md) owns the schema, menu, editing and defaults.

- A content renderer may prepare a replacement in a spare content iframe. It
  must not use a spare Workbench application document. Promotion waits for
  matching destination readiness; superseded loads cannot replace a newer selection
  or put outgoing pixels beneath the new destination's label.
- Retained preview sessions preserve their attached iframe
  across selections: one active session and up to three inactive
  sessions for 15 minutes after departure. Returning preserves form edits,
  scroll, menus, and application state. Inactive frames are inert, keep their
  layout dimensions, and cannot send navigation or keyboard commands to the
  active canvas. A loading replacement is protected until promoted or cancelled.
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
- The top bar exposes the page with its state or Storybook story picker, the
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
- Managed-preview readiness polls the host for up to 15 seconds before the
  frame swap proceeds anyway; visible images are awaited for at most 3 seconds
  ([workbench.js](../ui/legacy/workbench.js), [browser.js](../ui/preview/browser.js)).

## Implementation gaps

- **Dropped authored entries are invisible in VS Code.** Problems for dropped
  collections, groups, pages, and states go only to the canvas webview's
  console. `server/node/config.js` on the server deliberately skips them, so the sidebar's
  problems list never shows them.
- **No problems list in a standalone browser.** The canvas does not display
  `problems` from `/_workbench/config`; in a browser, catalog and preview
  failures are visible only by reading that route.
- **No sidebar problems when nothing is imported.** With `previews: false` and
  no catalog, the VS Code sidebar builds from the YAML without asking the
  server, so implementation and mapping problems are not shown there.
- **Some failures never become problems.** A stopped Storybook used only by
  mappings, a mapped Simulator whose device is not booted, and an authored
  entry with an invalid `src` or state id fail only on the canvas or are
  dropped silently; see [implementations](implementations.md#current-gaps) and
  [Storybook](storybook.md#current-gaps).

## Verification points

- [address.test.js](../ui/legacy/address.test.js) checks address parsing,
  writing, and round trips.
- [page-list.test.js](../ui/legacy/page-list.test.js) checks listed states and empty
  versus filtered lists.
- [config.test.js](../server/node/config.test.js) checks YAML reading, the local merge,
  and server-resolved paths and problems;
  [manifest.test.js](../contracts/manifest.test.js) checks the shared rules,
  including sizes.
- [preview.test.js](../ui/legacy/preview.test.js) checks spare-iframe
  replacement, superseded loads, warm managed-preview swaps, width modes, and
  story switching.
- [preview-host.test.js](../ui/legacy/preview-host.test.js) and
  [preview-runtime.test.js](../ui/preview/preview-runtime.test.cjs) check host reuse,
  disposal, hidden-iframe readiness, and lazy images.
- [preview.test.js](../server/node/preview.test.js) checks preview serving at definition
  paths, `previews: false`, and merging a discovered preview into an authored
  page.
- [server.test.js](../server/node/server.test.js) checks the config route and injection of
  `preview-compat.js` into served HTML.
- Not covered: no test checks that the sidebar and canvas show the same
  problems list.
