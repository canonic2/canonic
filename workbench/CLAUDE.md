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
- `capture*.js`, `electron-capture.js`, `capture-helper/`:
  screenshots. `window-stream.js`, `window-capture/`: the native window
  stream behind the iOS Simulator and window lenses.
- `electron-runtime.js`, `electron-runtime/`: the shared bundled Electron
  runtime for capture and preview compilation/server processes.
- `handoff.js`, `export.js`: handoffs and the design-system export.
  `remote.js`: requests to implementations.
- `workbench/`: the browser canvas: manifest reader, states, lenses,
  navigation, markup. Read `workbench/README.md` before changing it.
- `docs/`: the user guides, published on the website as they are.
- `design/`: the Workbench project's design pages, shown in this repository's
  workbench. They load the shipping `workbench/` code with sample data and
  aren't packaged.
- `specs/`: internal product requirements, workflows, decisions, and discoveries;
  these are kept outside the published documentation.
- `scripts/`: VSIX packaging, including the per-platform Electron runtime.

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
`package.json` and both entries at the top of `package-lock.json`, and adding
the version's entry to `CHANGELOG.md`: it becomes the release's notes on
GitHub and the website, and the release workflow refuses a tag without one.
Never push a tag without the user's go-ahead.

---

# Product specifications and documentation

Use `product-specs` to maintain internal requirements, workflows, system contracts,
decisions, discoveries, acceptance criteria, and known gaps. Use `product-docs`
to maintain reader-facing guides, references, and troubleshooting. The package's
writing rule defines its source locations, audience, publishing, and validation.

- Read the relevant specification before changing product behavior. Update it
  when a requirement, decision, workflow, or material discovery changes. Separate
  agreed requirements, proposals, verified behavior, implementation gaps, and
  open questions. Existing code or passing tests do not establish product intent.
- Record durable discoveries with evidence and rationale. Label hypotheses;
  include dates and conditions for measurements where relevant. Keep current
  requirements prominent rather than accumulating a conversation transcript.
- Update affected documentation when supported behavior changes. Verify claims,
  examples, labels, commands, defaults, and recovery instructions against the
  product. A specification does not establish that a proposed feature is available.
- Docs, READMEs, and agent instructions describe only what exists. When a
  feature, key, command, file, or route is removed or replaced, delete or rewrite
  the text about it; do not keep it with a note that it was removed, renamed, or
  is "no longer" supported, and avoid "previously", "formerly", and "now" phrasing.
  History belongs in the changelog, release notes, and commit messages.
- Keep internal requirements and investigation notes in specs. Include technical
  detail in docs when it helps the intended audience use, configure, integrate,
  or troubleshoot the product. Prefer existing topics and cross-links over copies.
- Check links and run the package's relevant validation. Do not add tests that
  merely assert documentation text. Writing a spec does not authorize implementing
  proposals; writing docs does not itself authorize publishing or distribution.

---

# Workbench writing locations and checks

- Internal specs: `packages/workbench/specs/`, indexed by `specs/README.md`.
- Public user guides: `packages/workbench/docs/`, indexed by `docs/README.md`.
  The audience is people using the extension and standalone Workbench.
- Astro reads all Markdown in `docs/` directly; keep specs outside that folder.
  Use one title heading, no required frontmatter, and relative `.md` guide links.
  `README.md` becomes an overview route. Source links outside docs become GitHub
  links. Preserve published URLs when reorganizing guides.
- Add new guides to the docs index and `docGroups` in
  `packages/website/src/lib/docs.js`; unlisted guides appear under "More guides".
  Verify generated navigation, previous/next links, fragments, and search.
- For public guide changes, run `pnpm --dir packages/website test` and
  `pnpm --dir packages/website run build`, then inspect affected pages. For
  Workbench behavior changes, run `npm --prefix packages/workbench test`.
  Markdown-only internal specs need link review and `git diff --check`.
- When visible behavior affects overview copy or screenshots, use
  `website-workbench`. Consuming projects supply configuration, not copies of
  Workbench scripts. Publishing follows the separately authorized release workflow.
