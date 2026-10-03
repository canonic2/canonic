# Core workbench contract

This contract applies to authored screens and imported catalog entries. The
implementation is in [workbench.js](../../workbench/workbench.js),
[nav.js](../../workbench/nav.js), [manifest.js](../../workbench/manifest.js),
[screens.js](../../screens.js), and [panel.js](../../panel.js).
The editor host and screenshot processes have separate
[extension](vscode-extension.md) and [capture](capture.md) contracts.

## Configuration and serving

- A project opts in with `workbench.yaml` at its workspace root. The extension
  serves the project at `/` and its bundled workbench at `/_workbench/` on one
  loopback origin. It does not copy workbench files into the project.
- `workbench.local.yaml` is optional. It replaces top-level values except that
  `implementations` merges by implementation name, one level deep. A nested
  `start` block supplied locally replaces the whole committed `start` block.
- The config accepts block YAML. Names and IDs used in addresses are checked;
  invalid entries are reported rather than silently converted into screens.
  The browser and server use the same implementation rules from `manifest.js`.
- `sections` contains ordered screen-list sections. Each section contains screens and
  optionally one level of folders. Imported catalog sections join matching
  authored sections by name. A failed catalog leaves usable authored screens
  available and adds a diagnostic.
- `GET /_workbench/config` exposes the server-resolved configuration, imported
  sections, source paths, and `problems`. The sidebar asks the server for
  imported catalogs because its VS Code webview cannot fetch them directly.

## Selection and navigation

- One selection is encoded as `#<src>[:<state>]@<width>[~<lens>]`. For example,
  `#pages/sign-in.html:error@393~staging` names a screen, state, frame width,
  and implementation. The default state and design lens are omitted. The
  workbench normalizes the address after routing to the view it actually shows.
- A screen picked in the VS Code sidebar travels to the workbench as `wb-go`.
  The workbench reports settled selections with `wb-here`. The sidebar follows
  changes made inside the canvas, including a local link followed in a live
  authored page. Picks wait while the embedded workbench refreshes its config.
- The sidebar lists the sections, then the chosen section's screens; the filter searches screens across sections,
  including state and folder labels. A screen with multiple states expands to
  state rows. An imported title is a disclosure row: its named states are the
  selectable stories. A single-state authored screen stays one row.
- A lens choice persists across screen picks. A screen without that lens shows
  its design. An imported implementation-only screen always uses its own lens.

## Canvas and frame

- The canvas offers Fit, desktop (1512 × 982), mobile (393 × 852), and
  Resizable modes. A screen's optional `viewports` list disables unsupported
  modes; omission enables all four. Fit follows the available workbench space.
  Fixed and resizable sizes keep their dimensions when the editor is smaller.
- An ordinary iframe navigation uses a spare iframe. The current preview stays
  visible until its replacement loads, then the frames exchange roles. A
  superseded load must not replace a newer selection.
- The top bar exposes the screen with its state or Storybook story picker, the
  effective lens, source files when resolved, and reload. Markup, screenshot,
  and handoff actions float over the canvas. Canvas zoom scales the frame
  without changing its layout size; captures use the real frame size. A preview from an external origin remains inside an iframe; the
  workbench does not read its DOM directly.

## Editor lifecycle and failure behavior

- There is one Workbench tab per VS Code window. The screen sidebar
  remains separate and follows the canvas. Reloading the VS Code window is
  required to activate a newly installed extension build.
- The sidebar watches `workbench.yaml` and `workbench.local.yaml`. Its rebuild
  asks an open canvas to refresh first, so a new row does not race an old
  manifest. The sidebar also offers **Refresh Screens**.
- Missing previews, invalid configuration, stopped implementations, and
  capture errors should give a named problem. A failure in one imported
  catalog must not remove valid handwritten screens.

## Verification points

- [address.test.js](../../workbench/address.test.js) checks address parsing.
- [nav.test.js](../../workbench/nav.test.js) checks row and state behavior.
- [panel.test.js](../../panel.test.js) and
  [sidebar.test.js](../../workbench/sidebar.test.js) check editor handoff and
  refresh ordering.
- [preview.test.js](../../workbench/preview.test.js) checks iframe replacement
  and story switching.
