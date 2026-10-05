# Website — Package Instructions

The Canonic website, served at `https://canonic.sh/`. It covers four products
(Workbench, Sandbox, Playground, Shield). Only Workbench has pages so far:
an overview (the home page), Docs, Install, and Changelog. `README.md`
explains the build, the screenshots, and publishing.

## Layout

- `src/pages/index.astro`: the Workbench overview.
- `src/pages/workbench/`: the Docs pages (built from the Workbench guides),
  the Install page, and the Changelog.
- `src/layouts/Base.astro`: the product bar, section nav, and footer.
- `src/lib/`: `workbench.js` (page URLs, nav, release data), `docs.js` (guide
  order and link rewriting), `releases.js` (release selection and notes),
  `marks.js` (logo and product marks), each with tests where they have logic.
- `src/styles/global.css` and `docs.css`: tokens and components.
- `src/data/screenshots.json`, `public/images/`, `screenshots/`: Workbench
  screenshots, their regions, and the fixture and script that capture them.

## Architecture direction

Apply shared module, UI, testing and diagnostics contracts using Astro, existing
JavaScript helpers and progressive enhancement. Pages compose content; components
render explicit inputs; lib helpers own reusable transformations and data access;
browser scripts own their listeners and interactions.

The current layout is the migration starting point. Extract repeated behavior
within the affected scope and preserve static rendering, base-path links and
source-owned product documentation. Use develop-module, review-module,
create-ui-component, write-tests, fix-checks and debug-runtime with the package's
commands. Do not introduce a client framework or shared UI package as a prerequisite.

## Rules

- Astro with static output and pnpm. No UI framework or client islands; a
  small `is:inline` script is fine for progressive enhancement, and pages must
  work without it. Add a dependency only when the user agrees.
- Every page uses `Base.astro`. Link files in `public/` through `base` from
  `src/lib/base.js`, never with a leading `/`.
- Every color is a token in `global.css` with light and dark values.
- The Workbench guides live in the extension's `docs/`. Edit them there, not
  on the site.
- The extension's `specs/` holds internal specifications and is not published.
- Versions, download links, and the changelog come from GitHub's releases at
  build time. Never hardcode a version.
- Links that leave the site open in a new tab (`target="_blank"
  rel="noopener"`) and carry a `↗`.
- Never hand-edit `src/data/screenshots.json`; `screenshots/capture.cjs`
  writes it. Use Acme and example.com in copy and screenshots, and keep copy
  plain and factual.
- Use only exported product marks; never redraw one.

## Commands

From this folder (or with `pnpm --dir packages/website` from the repository
root):

| Change | Command |
| --- | --- |
| Any change | `pnpm test` and `pnpm run build` |
| Local preview | `pnpm run dev` |
| Screenshots | `node screenshots/capture.cjs` (needs the extension beside this folder), then view `public/images/` |

## Publishing

Pushing a `website/v<major>.<minor>.<patch>` tag deploys the site. GitHub
Pages names each deployment after its commit, and a second deploy from an
already-deployed commit leaves the first one live. Put each website tag on a
commit that has never been deployed, never on a `workbench/v*` tag's commit.
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

# Website writing locations and checks

- Internal specs for the website itself: `packages/website/specs/`, indexed by
  `specs/README.md` when its first specification is written. These cover website
  behavior, navigation, content generation, accessibility, and product presentation;
  each product's own requirements stay in that product's specs.
- Reader-facing website content lives in `packages/website/src/pages/` and its
  existing content sources. `packages/website/README.md` is the maintainer guide
  for building, screenshots, and publishing. There is no separate website docs
  collection today; do not create duplicate content to fit a generic directory.
- Workbench user guides are owned by `packages/workbench/docs/` and loaded by
  `src/content.config.ts`. Edit those guides at their source. Website specs and
  maintainer notes must not enter the Workbench collection or search index.
- Follow `website-update` for site structure and `website-workbench` for the
  Workbench overview and screenshots. Match supported product capabilities;
  download and release data come from GitHub rather than invented versions.
- For site content or behavior changes, run `pnpm --dir packages/website test`
  and `pnpm --dir packages/website run build`, then inspect affected output.
  Follow the relevant website skill for visual checks when layout or screenshots
  change. Internal Markdown-only specs need link review and `git diff --check`.
- Writing content does not itself authorize deployment. Use `website-release`
  when publishing is requested.
