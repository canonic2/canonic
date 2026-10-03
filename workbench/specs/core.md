# Core workbench contract

This contract applies to every screen: authored pages, discovered TypeScript
previews, and imported catalog entries. The implementation is in
[workbench.js](../workbench/workbench.js), [nav.js](../workbench/nav.js),
[manifest.js](../workbench/manifest.js), [config.js](../workbench/config.js)
(browser), and [config.js](../config.js) (server). The editor host, preview
compilation, and screenshot processes have separate
[extension](vscode-extension.md), [TypeScript preview](previews.md), and
[capture](capture.md) contracts.

## Configuration and serving

- A project opts in with `workbench.yaml` at its workspace root. One loopback
  server serves the project at `/`, its bundled workbench at `/_workbench/`,
  and compiled TypeScript previews at their definition paths
  (`/<path>.workbench.ts`) with assets under `/_workbench/previews/`, all on
  one origin. Workbench does not copy files into the project.
- `workbench.local.yaml` is optional. It replaces top-level values except that
  `implementations` merges by implementation name, one level deep. A nested
  `start` block supplied locally replaces the whole committed `start` block,
  and a local `previews` value replaces the committed one.
- The config accepts block YAML. Names and IDs used in addresses are checked;
  invalid entries are reported rather than silently converted into screens.
  The browser and server use the same implementation, viewport, and `previews`
  rules from `manifest.js`.
- `sections` contains ordered screen-list sections. Each section contains
  screens and optionally one level of folders. `sections` may be omitted when
  previews or a catalog supply screens.
- `previews` is a map (`include`, `config`) or `false`. Discovery is on by
  default; `false` turns it off and no preview code runs. An invalid value is
  a problem and leaves previews off until fixed. Discovery, definitions, and
  the worker are specified in [TypeScript previews](previews.md).
- Discovered previews and imported catalog sections join authored sections
  with the same name. A discovered preview whose definition is also listed as
  an authored `src` stays one row: the authored label, lenses, and code
  pointers are kept, and the definition supplies its states.
- A failed catalog or preview compilation leaves usable authored screens
  available and adds a problem. One invalid preview definition does not hide
  the others.
- `GET /_workbench/config` answers with the server-resolved configuration,
  imported sections (`catalogSections`), absolute source paths, and
  `problems`. It is read fresh on every request. The VS Code sidebar asks the
  server for imported sections because its webview cannot fetch them directly.

## Problem reporting

- Every configuration, catalog, preview, and implementation problem names
  where it came from, for example
  `Pages › Auth › Sign in: state id “Error” must be kebab-case`.
- Every surface shows the same problems list (decided 2026-10-03): the VS Code
  sidebar above its screens, and the canvas wherever it runs (the editor tab,
  **Open Canvas in Browser**, and a standalone server). `/_workbench/config`
  lists them under `problems`.
- The list covers every entry dropped while reading the YAML (sections,
  folders, screens, states, and invalid `src` values), implementation,
  viewport, mapping, code-pointer, `previews`, catalog, and
  preview-compilation problems, and implementations that are not running,
  including a stopped Storybook used only by mappings and a mapped Simulator
  device that is not booted. See
  [the extension contract](vscode-extension.md#sidebar-and-problems).
- Missing previews, invalid configuration, stopped implementations, and
  capture errors give a named problem. A failure in one imported catalog must
  not remove valid authored screens.

## Selection and navigation

- One selection is encoded as `#<src>[:<state>]@<width>[~<lens>]`. For example,
  `#pages/sign-in.html:error@393~staging` names a screen, state, frame width,
  and implementation. The default state and design lens are omitted; the
  workbench always writes the width. It normalizes the address after routing
  to the view it actually shows.
- The sidebar lists the sections, then the chosen section's screens; the
  filter searches screens across sections, including state and folder labels.
  A screen with multiple states expands to state rows. An imported title is a
  disclosure row: its named states are the selectable stories. A single-state
  authored screen stays one row.
- A lens choice persists across screen picks. A screen without that lens shows
  its design. An imported implementation-only screen always uses its own lens;
  a discovered TypeScript preview uses its **Workbench** lens.
- In VS Code, sidebar picks and canvas selections travel as `wb-go` and
  `wb-here`; the [extension contract](vscode-extension.md#editor-surfaces-and-messages)
  defines the relay and its ordering.

## Canvas and frame

- Config and catalog discovery show **Loading screens…**. The first iframe
  preview shows **Loading preview…** until it is ready; later navigations keep
  the outgoing preview visible. Loading indicators honor reduced motion.
- The canvas offers Fit, Laptop (1512 × 982), Mobile (393 × 852), and
  Resizable modes. A screen's optional `viewports` list disables unsupported
  modes; omission enables all four. Fit follows the available workbench space.
  Fixed and resizable sizes keep their dimensions when the editor is smaller.
- An ordinary iframe navigation uses a spare iframe. The current preview stays
  visible until its replacement loads, then the frames exchange roles. A
  superseded load must not replace a newer selection.
- Managed previews (same-origin `.workbench.ts` and `.workbench.tsx` pages)
  use the same two frames, each holding an initialized preview host. The
  host's compatibility bridge and shared lifecycle runtime stay loaded while
  adapter content is mounted into the spare, then promoted. The outgoing
  adapter is disposed and its document reset for reuse; no destination page is
  predicted or preloaded. This path is shared by every built-in and custom
  adapter.
- Managed previews wait for mounting, fonts, and visible images before becoming
  ready. Offscreen lazy images keep native lazy loading and do not delay the
  frame swap. Readiness must also progress when the browser suspends animation
  frames in the hidden loading iframe.
- The top bar exposes the screen with its state or Storybook story picker, the
  lens, frame widths, reload, source files when resolved, a copyable
  reference, **Open on its own**, preview controls for a ready TypeScript
  preview, and **More** (page configuration and design-system export when the
  server is running). Narrow canvases fold secondary actions into **More**.
  Markup, screenshot, and handoff actions float over the canvas.
- Canvas zoom scales the frame without changing its layout size; captures use
  the real frame size. A preview from an external origin remains inside an
  iframe; the workbench does not read its DOM directly.

## Verified behavior

Checked against the working tree on 2026-10-03.

- `previews` validation: `manifest.previews` returns `false` and adds a
  problem for a non-map, an empty or escaping `include`, or an absolute or
  escaping `config` ([manifest.js](../workbench/manifest.js)).
- Merging a discovered preview into an authored row is done by
  `manifest.mergeSections`, used by both the sidebar and the canvas.
- Managed-preview readiness polls the host for up to 15 seconds before the
  frame swap proceeds anyway; visible images are awaited for at most 3 seconds
  ([workbench.js](../workbench/workbench.js), [browser.js](../preview/browser.js)).

## Implementation gaps

- **Dropped authored entries are invisible in VS Code.** Problems for dropped
  sections, folders, screens, and states go only to the canvas webview's
  console. `config.js` on the server deliberately skips them, so the sidebar's
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
  dropped silently; see [other previews](other-previews.md#current-gaps) and
  [Storybook](storybook.md#current-gaps).

## Verification points

- [address.test.js](../workbench/address.test.js) checks address parsing,
  writing, and round trips.
- [nav.test.js](../workbench/nav.test.js) checks state rows, imported titles,
  and empty versus filtered lists.
- [config.test.js](../config.test.js) checks YAML reading, the local merge,
  and server-resolved paths and problems;
  [manifest.test.js](../workbench/manifest.test.js) checks the shared rules,
  including viewports.
- [preview.test.js](../workbench/preview.test.js) checks spare-iframe
  replacement, superseded loads, warm managed-preview swaps, width modes, and
  story switching.
- [preview-host.test.js](../workbench/preview-host.test.js) and
  [preview-runtime.test.js](../preview-runtime.test.js) check host reuse,
  disposal, hidden-iframe readiness, and lazy images.
- [preview.test.js](../preview.test.js) checks preview serving at definition
  paths, `previews: false`, and merging a discovered preview into an authored
  row.
- [server.test.js](../server.test.js) checks the config route and injection of
  `preview-compat.js` into served HTML.
- Manual: in VS Code, add an invalid state id and confirm the problem appears
  only in the canvas console (the first gap above).
