# Workbench — Package Instructions

Workbench is a VS Code extension that puts a project's screens on one canvas
at real device widths, with markup, screenshots, and a handoff to an agent. A
project's whole setup is its `workbench.yaml`, plus an optional, ignored
`workbench.local.yaml`.

## Layout

- `extension.js`, `panel.js`, `startup.js`: the extension, its webview, and
  the start of configured implementations.
- `server.js`, `config.js`, `screens.js`, `yaml.js`: the local HTTP server on
  127.0.0.1 (port 3579 and up) and the config it serves.
- `capture*.js`, `electron-capture.js`, `capture-helper/`, `capture-runtime/`:
  screenshots. `window-stream.js`, `window-capture/`: the native window
  stream behind the iOS Simulator lens.
- `handoff.js`, `export.js`: handoffs and the design-system export.
  `remote.js`: requests to implementations.
- `workbench/`: the browser canvas: manifest reader, states, lenses,
  navigation, markup. Read `workbench/README.md` before changing it.
- `docs/`: the user guides, published on the website as they are.
- `scripts/`: VSIX packaging, including the per-platform capture runtime.

## Rules

- The workbench ships inside the extension. Projects supply only
  `workbench.yaml`; never copy the workbench into a project or add its scripts
  to a project's pages.
- Keep the code project-agnostic, in plain HTML, CSS, and JavaScript, matching
  the module style of each file. Use Acme and example.com in fixtures.
- `workbench/manifest.js` supplies the manifest rules to both the browser and
  the server. Keep the two readers consistent.
- When the `workbench.yaml` schema changes, update `workbench/README.md` and
  `docs/configuration.md` in the same change, along with the agent skill and
  rule that describe the schema (the root guide names them).
- `docs/` describes visible controls and behavior. When either changes,
  update the affected guide, and the website's Workbench pages.
- Test behavior with the existing Node tests and fixtures, next to the module
  they cover.

## Commands

From this folder (or with `npm --prefix packages/workbench` from the
repository root):

| Change | Command |
| --- | --- |
| Extension or workbench behavior | `npm test` |
| Run the server on a project | `node server.js <project>`, then open the reported workbench URL |
| Packaging | `npm run package` (this host) or `npm run package:all -- <target>`, then inspect `dist/` |
| Locked dependencies | `npm ci --ignore-scripts` |

Packaging downloads a pinned Electron build of roughly 120–155 MB per target.
Builds made for another platform are unsigned and untested there. Keep
temporary fixtures and captures in a temporary directory.

Releases are cut from a `workbench/v<version>` tag after bumping `version` in
`package.json` and both entries at the top of `package-lock.json`. Never push a
tag without the user's go-ahead.
