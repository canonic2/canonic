# Studio — Package Instructions

Studio is a local Electron desktop workspace with project tabs and concurrent
task sessions. Each managed session owns a Git worktree, IDE profile, host
process group, ports and optional Docker Compose stack. Read README.md,
specs/README.md and the relevant topic specification before changing behavior.

## Source ownership

- src/application/: composition and workflows across feature modules.
- src/modules/: configuration, Git, projects, sessions, state, runtime and IDE
  services. Keep validation and lifecycle ownership with their feature.
- src/platform/: shared paths, process groups, errors and queues.
- app/desktop/: Electron windows, workspace views, commands and the IPC boundary.
- app/renderer/ and app/studio.ts: shell presentation and action orchestration.
- app/preload.cts: sandboxed CommonJS bridge.
- config/project.schema.json: versioned project recipe contract.
- scripts/: setup, build, launch and engineering check orchestration.
- test/: behavioral suites, independent full-stack fixture and desktop runners.
- specs/: internal contracts and acceptance evidence; docs/: reader-facing guides.

## Architecture direction

Apply the shared module, UI, testing and diagnostics contracts with Studio's
pnpm/TypeScript toolchain. Feature services own domain state and lifecycle;
application code coordinates them; desktop code adapts Electron; renderer code
receives display data and invokes explicit commands.

Use develop-module and review-module for structure, create-ui-component for
presentation, write-tests for coverage, fix-checks for engineering failures and
debug-runtime for diagnosis. The Studio development and validation skills add
session/resource acceptance requirements. Preserve module ownership and extend
boundaries as capabilities grow; a shared UI package remains a separate decision.

## Development commands

Run these from packages/studio, or use pnpm --dir packages/studio from the
repository root. This package has its own frozen lockfile and no root workspace.
For agent shell calls, use the repository-root pnpm --dir packages/studio form;
Shield's command allowances are scoped to that package path.

| Task                                     | Command                                |
| ---------------------------------------- | -------------------------------------- |
| Install locked dependencies              | pnpm install --frozen-lockfile         |
| Install the pinned IDE runtime           | pnpm run setup                         |
| Inspect prerequisites                    | pnpm run doctor                        |
| Launch the desktop                       | pnpm start                             |
| Standard engineering gates               | pnpm run check                         |
| Full desktop and Docker/Turbo acceptance | pnpm run check:full                    |
| Focused behavioral suite                 | pnpm test                              |
| Automatic formatting                     | pnpm run format                        |
| Check a project recipe                   | pnpm run config:check /path/to/project |

The full check needs a desktop, the installed IDE, Git, Docker with Compose and
network access. Missing prerequisites must be reported; do not treat skipped
integration checks as acceptance. For instruction-only changes, regenerate Shield
from the repository root with node shield/src/index.mjs and inspect the outputs.

## Agent configuration sources

Edit these instructions in .canonic/guides/studio/ at the repository root.
Studio rules live in `.canonic/rules/studio-*.md` and workflows in
`.canonic/skills/studio-*/SKILL.md`. Regeneration writes package instructions and
Claude/Codex skills. Generated guides and skills are not separate source files.

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

# Studio implementation and isolation

- Use pnpm and the pinned TypeScript toolchain on Node 24+. Author ESM source
  and Node scripts as .ts with explicit relative extensions; use .cts for the
  sandboxed preload. Edit source, not dist or downloaded IDE code.
- Keep feature modules independent of application orchestration and Electron.
  Keep project/session models independent of runtime and IDE launchers.
  Renderer runtime imports cannot use Node or Electron. Preserve sandbox,
  context isolation, trusted-frame checks and input validation at the IPC boundary.
- Project IDs and session IDs determine ownership. Main references the user's
  original checkout; task sessions use managed Git worktrees. Preserve dirty
  files, worktrees and branches across Stop, project close and application quit.
- Scope runtime queues, process groups, ports, IDE profiles, extensions and
  workspace views to each session. Switching selection must not transfer or stop
  another session's resources. Provision commands must leave no descendants on
  completion, failure or timeout.
- Resolve and validate Compose configuration before startup; retain its saved
  plan for cleanup. Keep host bindings on allocated loopback ports and mutable
  resources session-scoped. Stop retains named volumes. Cleanup uses recorded
  ownership, never the current UI selection, names alone or Docker-wide prune.
- Keep persisted state updates serialized and atomic; preserve Git rollback and
  cleanup error reporting. Do not silently reset malformed state or claim crash
  recovery from in-memory ownership.
- Keep fixtures under test and runtime reports under ignored artifacts.
  Production startup must not import fixtures or verification runners. Use
  separate desktop test profiles and preserve user projects.
- Run pnpm run check for implementation changes. Changes to session isolation,
  lifecycle, IPC, IDE or runtime behavior also require relevant real integration
  suites; run pnpm run check:full for acceptance. Keep coverage scope and
  thresholds in specs/quality-checks.md honest; do not bypass checks to hide a
  regression. Report host and prerequisite limits with the checks actually run.

---

# Studio writing locations and checks

- Use product-specs for internal contracts in packages/studio/specs/, indexed
  by specs/README.md. Keep intended behavior, verified evidence, known limits
  and proposed work distinct.
- Use product-docs for reader-facing guides in packages/studio/docs/, indexed
  by docs/README.md and the package README. Readers set up local projects, use
  project/session controls and maintain the desktop application.
- These are repository Markdown documents; Studio has no website publishing
  integration. Use relative links and supported commands, labels and recipe
  keys. Use specs for design directions rather than claiming them in guides.
- Recipe changes require config/project.schema.json, configuration examples,
  affected specs and docs to agree. Verify examples without executing arbitrary
  project commands. Link to the independent full-stack fixture for complete
  Express/Vite/Turbo setup rather than duplicating it.
- For package Markdown changes, run pnpm --dir packages/studio run
  check:contracts and pnpm --dir packages/studio run format:check, review links
  and heading fragments, and run git diff --check. Runtime changes require the
  relevant engineering and integration checks described in specs/quality-checks.md.
- Edit Shield guide/rule/skill sources under .canonic/ and run
  node shield/src/index.mjs from the repository root. Inspect the generated
  Studio AGENTS.md/CLAUDE.md and Claude/Codex skills. Instruction-only changes
  do not require desktop builds, external distribution or publishing.

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
