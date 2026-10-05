# Workbench — Package Instructions

Workbench is a VS Code extension that puts a project's pages on one canvas
at real device widths, with annotations, screenshots, and a handoff to an agent. A
project's whole setup is its `workbench.yaml`, plus an optional, ignored
`workbench.local.yaml`.

## Layout

- `extension.js`, `panel.js`, `startup.js`: the extension, its webview, and
  the start of configured implementations.
- `server.js`, `config.js`, `sidebar-view.js`, `yaml.js`: the local HTTP server on
  127.0.0.1 (port 3579 and up) and the config it serves.
- `capture*.js`, `electron-capture.js`, `capture-helper/`:
  screenshots. `window-stream.js`, `window-capture/`: the native window
  stream behind the iOS Simulator and window lenses.
- `electron-runtime.js`, `electron-runtime/`: the shared bundled Electron
  runtime for capture and preview compilation/server processes.
- `handoff.js`: handoffs. `remote.js`: requests to implementations.
- `src/modules/export/`: the TypeScript design-system source and archive
  exporter, exposed through `index.ts`. Job orchestration remains in `server.js`.
- `src/modules/docs/`: docs, a lens of any page: which lens a page shows
  (`canvas/lenses.ts`), the docs canvas, the docs page script, and the server's
  docs service, exposed through `index.ts`.
- `src/`: TypeScript source. Current capability directories include
  `src/sizes/` and `src/canvas/`; shared presentation and
  infrastructure live in `src/components/`, `src/theme/` and `src/server/`.
- `workbench/`: the browser canvas: manifest reader, states, lenses,
  page list, annotations. Read `workbench/README.md` before changing it.
- `docs/`: the user guides, published on the website as they are.
- `design/`: the Workbench space's design pages, shown in this repository's
  workbench. They load the shipping browser code with sample data and
  aren't packaged.
- `specs/`: internal product requirements, workflows, decisions, and discoveries;
  these are kept outside the published documentation.
- `scripts/`: VSIX packaging, including the per-platform Electron runtime.

## Architecture direction

Apply the shared module, UI, testing and diagnostics contracts. Browser canvas
code, extension/host transport, server I/O and manifest interpretation have
different responsibilities; keep those boundaries explicit.

New code is TypeScript under `src/`, run without a build step:

- Node 24 loads `.ts` by stripping types, and the extension requires VS Code
  1.123 or later for the same reason. Use erasable syntax only (no `enum`,
  `namespace`, parameter properties, or `import =`), `import type` for types,
  and explicit `.ts` extensions on relative imports.
- `src/` is ES modules (`src/package.json` sets `"type": "module"`). The
  CommonJS host files `require()` them directly.
- Browser modules in `src/` are served with their types stripped by the local
  server; the canvas never loads a compiled copy.
- The direct VS Code sidebar uses `src/server/webview-components.ts` to bundle
  the component bootstrap in memory with the packaged compiler, authorized by
  a per-document CSP nonce. Component styles use adopted stylesheets; shared
  theme CSS loads as a local webview resource. No compiled copy is maintained.
- Keep pure logic (validation, parsing, matching) free of Node, DOM, and
  transport so it is tested directly; filesystem, HTTP, compiler, and DOM code
  are adapters around it.

### Capability module locations

New capability modules and substantial capability migrations belong under
`src/modules/<capability>/`, such as `src/modules/export/`. Keep each
capability's domain logic, workflows, dedicated filesystem/HTTP/compiler/browser
adapters, types and behavioral tests together. Expose deliberate public entry
points; callers use those contracts rather than another module's private files.
Browser-safe entry points must not import Node or host-only adapters.

Shared Web Components stay under `src/components/`, shared themes under
`src/theme/`, and shared server transport and asset serving under `src/server/`.
Application entry points compose these owners. A controller dedicated to one
capability belongs with that module; rendering remains in its component.

The existing capability directories and JavaScript host files are the migration
starting point. Move a capability into `src/modules/` when the requested change
substantially reworks it, in bounded steps that preserve public contracts; local
fixes stay in place. Update consumers, runtime imports, browser asset allowlists,
webview loading/CSP and packaging references together when paths change. Preserve
browser/server manifest agreement, host compatibility and packaged assets.
Use develop-module, review-module, create-ui-component, write-tests, fix-checks
and debug-runtime with the commands below. A shared UI package is not required.

### Browser UI direction

The target for Workbench-owned UI is Web Components, defined in
[`specs/web-components.md`](specs/web-components.md), with host-independent themes
defined in [`specs/themes.md`](specs/themes.md). Read both before component or theme
work, and use `workbench-ui` with the shared component/review workflows.

New components and substantial migrations belong under `src/components/<name>/`
as TypeScript custom elements with `wb-` tags, open Shadow DOM, explicit inputs,
typed events and owned connection cleanup. Shared theme assets belong under
`src/theme/`; components consume semantic `--wb-*` tokens with standalone defaults.
Host adapters supply VS Code theme values, including live changes, across the
canvas iframe boundary. Project preview frameworks and styling remain independent.

These are target locations and contracts, not a claim that the legacy shell has
been migrated. Local fixes can stay in place; migrate bounded controls and their
consumers when substantially reworking them. Check server asset allowlists,
webview resource loading/CSP and packaging when introducing new browser paths.

Every component has a docs page in this repository's Workbench space, added or
updated in the same change as the element: `design/<name>.md`,
`design/<name>.examples.ts` and a `defineDocs` `design/<name>.workbench.ts`
listed under **Components** in its interface area's group (Sidebar, Top bar, …). Examples use the shipping component's public API,
one per meaningful state, and the page ends with its Public API.
[`specs/web-components.md`](specs/web-components.md#design-pages) defines the
page; `src/components/design-pages.test.ts` fails a component without one.

## Rules

- The workbench ships inside the extension. Projects supply only
  `workbench.yaml`; never copy the workbench into a project or add its scripts
  to a project's pages.
- Keep the code project-agnostic. Write new code in TypeScript under `src/`;
  edit the existing JavaScript, HTML, and CSS in the module style of each file.
  Use Acme and example.com in fixtures.
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

The package uses pnpm, with its own frozen lockfile, a flat (`hoisted`)
`node_modules`, and no dependency install scripts. Run commands from this
folder, or with `pnpm --dir packages/workbench` from the repository root;
agent shell calls use the repository-root form, which Shield allows.

| Change | Command |
| --- | --- |
| Extension or workbench behavior | `pnpm test` |
| Type-check `src/` | `pnpm run check` |
| Run the server on a project | `node server.js <project>`, then open the reported workbench URL |
| Packaging | `pnpm run package` (this host) or `pnpm run package:all <target>`, then inspect `dist/` |
| Locked dependencies | `pnpm install --frozen-lockfile` |

Packaging downloads a pinned Electron build of roughly 120–155 MB per target.
Builds made for another platform are unsigned and untested there. Keep
temporary fixtures and captures in a temporary directory.

Releases are cut from a `workbench/v<version>` tag after bumping `version` in
`package.json` and `preview/package.json` (which the release publishes to npm
as `@canonic2/workbench`), and adding
the version's entry to `CHANGELOG.md`: it becomes the release's notes on
GitHub and the website, and the release workflow refuses a tag without one.
Never push a tag without the user's go-ahead.

---

# Behavioral testing and package verification

- Read the package guide and existing tests before choosing a runner, location
  or fixture. Use package scripts and supported runtime versions; do not introduce
  a new test framework simply to apply a shared workflow.
- Test observable contracts, success, meaningful failures and edge cases.
  Avoid tests that repeat implementation details, documentation text or generated
  file contents without exercising a behavioral contract.
- Test pure validation and transformations directly. Mock external boundaries
  when appropriate for isolation; do not mock away the behavior being verified.
- Use real temporary files, Git worktrees, processes, browser hosts or containers
  when those boundaries determine correctness. A unit mock cannot prove resource
  isolation, host compatibility or descendant cleanup.
- Fixtures are deterministic where practical and use neutral Acme/example.com
  data. Give integration resources unique ownership; cleanup only resources
  created by the run. Preserve user checkouts, profiles, processes and data.
- Restore mutated globals, environment, timers and test state. Keep production
  startup independent of fixtures. Store generated evidence in the package's
  ignored artifact locations or temporary directories.
- Select checks from the affected contract. Preserve the assertions and coverage
  scope while fixing failures; do not weaken thresholds, skip suites or broadly
  suppress rules to obtain a pass.
- Report executed commands and observed outcomes. Missing prerequisites, optional
  captures and platform limits are distinct from behavior failures and passing
  acceptance. Do not infer untested platform coverage from another host's result.

---

# Debugging from observed evidence

- Identify the affected package, host, identity and reproduction before editing.
  A deterministic failing test or supplied diagnostic may establish reproduction.
- Inspect existing logs, errors, state snapshots, reports and ownership records
  first. Use configured telemetry only when available and relevant; this rule
  does not require a telemetry service.
- Trace the execution path and correlate evidence using relevant project/session,
  request, run or resource IDs. Distinguish observed facts from hypotheses.
- Add temporary instrumentation only where existing evidence is insufficient.
  Keep it bounded; never expose credentials, secret environment values, full
  sensitive payloads or resolved configuration containing secrets.
- Verify the correction on the original failing surface and check neighboring
  owners when shared resources are involved. Preserve causes and cleanup errors.
- Remove temporary instrumentation when verification is complete unless it is
  deliberately retained as useful, documented diagnostics. Report remaining
  uncertainty rather than claiming a speculative fix.

---

# Modular architecture and incremental migration

These are the target architecture principles for Canonic packages. Package
guides describe current layouts, runtimes and compatibility constraints.

- Organize capabilities into modules with one clear owner for behavior, types,
  validation, state and lifecycle. Group files by responsibility; do not split
  every function into a file or create empty architectural layers.
- Use `src/modules/<capability>/` for new capability modules and bounded
  capability migrations by default. Keep the capability's logic, orchestration,
  dedicated adapters, types and tests together, following the package's test
  convention. A module directory represents an owned capability, not one file
  or every JavaScript/TypeScript importable unit.
- Keep application composition, shared presentation, themes and host/platform
  infrastructure outside `src/modules/` in the locations defined by the package
  guide. Capability-specific adapters stay with their module; infrastructure
  shared across capabilities has its own explicit owner. Do not create empty
  folders or move code into a generic utilities directory to tidy the tree.
- Package guides document current locations, target locations and justified
  layout exceptions. Existing directories outside `src/modules/` are not an
  instruction to move them all at once. Migrate within the requested capability,
  preserving public entry points and updating consumers, asset serving and
  packaging where paths change.
- Separate domain decisions, application workflows, external adapters and
  presentation. Pure logic must not depend on UI, transport or host frameworks.
  Application code coordinates capabilities; adapters implement external I/O.
- Expose deliberate module APIs. Depend on another module's public contract,
  not its internal state or incidental files. Keep shared helpers small and
  independent of product-specific behavior; avoid a miscellaneous shared module.
- Give mutable state one owner. Derive display data instead of maintaining
  competing copies. Scope caches and state to the identity and lifetime they serve.
- Validate untrusted input at boundaries. Separate reusable validation from
  HTTP, IPC, CLI and UI handling; keep domain invariants with their owner.
- Make filesystem, processes, network and persistence side effects explicit.
  Document ownership, completion, cancellation, cleanup and partial failure.
  Preserve error causes; observe promises and do not hide failures in empty catches.
- Compose dependencies at application entry points. Avoid hidden I/O or lifecycle
  initialization during imports. Prefer explicit dependencies over global service
  lookup; functions and classes are both valid when their responsibility is clear.
- Match each package's JavaScript/TypeScript module format, supported runtime,
  build and dependency constraints. Shared rules do not mandate a framework,
  dependency-injection library, package manager or repository-wide toolchain.
- Extract shared implementations after repeated behavior establishes a compatible
  contract. Shared instructions do not require shared code or a new package.
- New capabilities follow this direction. Improve separation within the affected
  area during normal changes; existing debt does not authorize unrelated rewrites.
  Larger migrations need a bounded scope, preserved public contracts, behavioral
  verification and updates to the owning package's specs and guides.
- Reviews distinguish existing debt from newly introduced violations. Document
  justified exceptions and concrete migration boundaries rather than claiming
  the whole package already follows the target architecture.

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

# Shared UI component and interaction contracts

Apply these contracts to interactive presentation code. Package guides supply
the actual component format, styles, host integration and verification commands.

- Components receive explicit inputs and expose events or callbacks. Rendering
  does not own Git, Docker, persistence or transport operations. Feature code
  coordinates those operations and passes display state to the component.
- Separate reusable interaction behavior from product-specific decisions.
  Keep listeners, observers, timers and transient state owned by the component;
  dispose them on unmount, replacement or shutdown as its host requires.
- Prefer native controls and structural semantics. Give controls accessible names,
  labels and visible focus; placeholders do not replace labels. Keep selected,
  expanded, busy and disabled semantics consistent with observable behavior.
- Preserve native keyboard behavior. Scope shortcuts to the owning widget and
  respect editable fields and embedded editors. Composite widgets need a defined
  keyboard model; do not invent tab or menu roles without implementing that model.
- Opening a dialog moves focus into useful content, contains focus as appropriate
  and restores the trigger on close. Escape and outside-click handling must not
  compete with other widgets or host shortcuts.
- Updates preserve meaningful focus, input, selection and scroll. Background
  refreshes should keep usable content visible. Define loading, empty, error,
  pending and disabled states; prevent accidental duplicate actions.
- Match the package's existing visual vocabulary and tokens. Keep changes targeted
  and copy action-oriented. Do not invent content, introduce a styling framework
  or impose a common theme as part of following these contracts.
- Verify interactions with keyboard and rendered behavior, including cleanup and
  failure states. Screenshots supplement assertions; they do not establish focus,
  accessibility or resource lifecycle correctness.

---

# Workbench UI architecture

Apply to Workbench-owned browser presentation, not the frameworks of previewed
projects. The target contracts are `packages/workbench/specs/web-components.md`
and `packages/workbench/specs/themes.md`; read them before component or theme work.

- New components and substantial component migrations use autonomous Web
  Components in `src/components/<name>/`, written in erasable TypeScript. Use
  `wb-` tags, open Shadow DOM, owned styles, explicit inputs, and typed public
  events. Register at browser entry points; keep DOM modules out of Node imports.
- Controllers own application state and product operations. Elements own rendering
  and interaction, release connection resources on disconnect, and reconnect
  without duplicate listeners. Preserve focus/input on updates and never reach
  into another element's private DOM or rely on document-wide IDs.
- Components consume semantic `--wb-*` tokens. Shared theme assets under
  `src/theme/` own complete standalone defaults and host mappings. VS Code colors
  are an adapter input, not a requirement for running a component.
- CSS variables cross Shadow DOM boundaries, not iframe documents. The embedding
  webview owns validated theme forwarding, initial readiness, live updates and
  snapshot replacement. Keep themes document-scoped and out of project previews,
  captures, configuration and server state.
- Verify native semantics, keyboard and focus in a real browser; cover defaults,
  custom host colors and high contrast. Check direct webview and server asset
  loading, CSP, and packaging for migrated components and styles.
- Every element in `src/components/<name>/` has its docs page in the Workbench
  space, in the same change: `design/<name>.md` with one section per meaningful
  state and a **Public API** section, `design/<name>.examples.ts` rendering the
  shipping element through its public inputs, and `design/<name>.workbench.ts`
  (`defineDocs`, `components/<name>`, `Components/<Area>/<Label>`, grouped by the interface area it appears in). A changed API or
  visible state updates the page with it. Examples import only what they draw.
  The contract is `specs/web-components.md#design-pages`; the
  `design-pages.test.ts` check fails a component without one.
- Existing vanilla UI is migration debt, not evidence that the target is shipped.
  Local fixes may remain in place; migrate bounded controls when substantially
  reworking them. Do not combine a component task with an unsolicited full UI
  rewrite, a new rendering framework, or changes to project preview adapters.

Use `workbench-ui` with `create-ui-component` for implementation or
`review-module` for review. These instructions refine the shared UI contracts;
they do not distribute a UI package or configuration to consuming projects.

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
  `packages/website/src/lib/doc-groups.js`; unlisted guides appear under "More guides"
  and have no state in the Website project's Docs preview.
  Verify generated navigation, previous/next links, fragments, and search.
- For public guide changes, run `pnpm --dir packages/website test` and
  `pnpm --dir packages/website run build`, then inspect affected pages. For
  Workbench behavior changes, run `pnpm --dir packages/workbench test`.
  Markdown-only internal specs need link review and `git diff --check`.
- When visible behavior affects overview copy or screenshots, use
  `website-workbench`. Consuming projects supply configuration, not copies of
  Workbench scripts. Publishing follows the separately authorized release workflow.
