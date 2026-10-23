# Workbench — Product Instructions

Workbench puts a project's pages on a browser canvas at real device widths,
with annotations, screenshots and agent handoffs. Projects supply
`workbench.yaml` and optional ignored `workbench.local.yaml`.

## Layout and ownership

`packages/workbench/` is the product root. No application owns the other parts.

| Location | Owner |
| --- | --- |
| `extension/` | VS Code activation, commands, sidebar/webview integration and extension manifest |
| `server/` | Rust Cargo package: the Workbench server (HTTP, project services, page index and preview compiler); `server/node/` owns its host, the standalone command and the Node evaluator |
| `capture/` | Electron screenshot helper (`app/`), runtime acquisition (`runtime/` bundle), window capture and native stream lifecycle |
| `ui/` | Browser-native JavaScript controllers, Web Components (`src/components/`), themes (`src/theme/`), canvas/content documents (`legacy/`), preview adapters (`preview/`) and portable viewer (`viewer/`) |
| `sdk/` | Published `@canonic2/workbench` definition helpers and authoring types |
| `contracts/` | Shared manifest/YAML interpretation and pure data/rules; browser runtime modules are JavaScript with declarations for backend TypeScript consumers |
| `scripts/` | Product checks, temporary VSIX assembly, per-platform builds and releases |
| `design/` | Internal design pages loading shipping components; not packaged |
| `specs/` | Internal requirements, architecture, acceptance evidence and implementation gaps |
| `docs/` | User guides read directly by the website |
| `labs/` | Bounded experiments following `specs/system/labs.md`; production code never depends on them |

Read `ui/legacy/README.md` before changing the browser shell, and
`specs/system/modules.md` for the ownership and dependency contracts. `server/README.md`
owns Rust layout and tooling. `workbench-server serve` is a space's whole
HTTP server: it owns the public port, keeps the page index resident, compiles
previews with Rspack's Rust crates and answers every route; project JavaScript
runs in its Node evaluator. Its host (`server/node/src/modules/host/`) answers
what belongs to the editor and the capture helper over stdio; `specs/system/server.md`
owns the process model and protocol.

## Engineering contracts

- The UI is independent of the backend. Read `specs/system/modules.md#ui-and-backend-independence`.
  The UI owns components, page-list/artboard presentation, navigation and rendering.
  The backend provides project data, operations and previewed content through APIs;
  backend domain services must not import UI runtime code, initialize components,
  or depend on their DOM or presentation state.
  Hosts load the UI and inject a data/operation client. The UI is plain JavaScript,
  Web Components, HTML and CSS, ready for direct browser use. Do not add TypeScript,
  transpilation, bundling or generated source copies to UI delivery. Hosts serve
  or copy authored assets unchanged; project preview compilation remains separate.
- Read `specs/system/rendering-flow.md`: one Workbench shell owns the canvas. Each
  artboard loads only project content through a dedicated renderer, never the
  application `index.html`, shell bootstrap, or a full shell hidden with CSS.
  Backend compilation/rendering of project React, docs and Storybook is expected.
  Preserve warm content sessions, independent artboard state and existing outer UI.
- Put new Node code in the owning part's `src/` as erasable
  TypeScript. Use `import type`, explicit `.ts` relative imports, and no enums,
  namespaces, parameter properties or import assignments. Node 24 strips types;
  VS Code requires 1.123 or later. Each TypeScript source root declares ESM;
  existing CommonJS hosts can require those entry points directly.
- Capability modules use `<part>/src/modules/<capability>/`; the Node host uses
  `server/node/src/modules/`. Rust uses Cargo's standard `server/src/` layout.
  Keep capability adapters with their owner, application composition at entry
  points, shared browser components in `ui/src/components/`, and themes in
  `ui/src/theme/`. Existing capability directories are bounded migration debt.
- Pure rules import no DOM, Node, VS Code or Electron. `contracts/` has only
  demonstrated cross-part rules and data, not application state or a utilities
  dumping ground. Browser source never imports a host service, even for types.
- The extension owns editor integration and the sessions it launches; server
  services own project state and their subordinate compiler processes. Capture
  owns rendering/native capture resources. Explicit lifecycle contracts cover
  startup, errors, cancellation and shutdown.
- UI-owned tooling supplies browser-executable assets independently of backend
  services and the project-preview compiler. Host infrastructure loads those assets;
  generic static delivery does not make the application backend their owner.
  Verify independent fixture-client UI loading and backend startup without UI files.
  Update imports, host resource URLs/CSP, UI tooling, packaging and source links together.
- The extension hosts the sidebar and supplies CSP/resource roots and editor
  capabilities. It loads the authored sidebar module directly. Components use adopted stylesheets;
  validated theme forwarding remains host-owned and project previews stay independent.
- `contracts/manifest.js` supplies browser/server manifest rules. Schema changes
  update `ui/legacy/README.md`, `docs/configuration.md`,
  `shared/skills/canonic-workbench/SKILL.md` and
  `shared/rules/canonic-workbench.md` together.
- Every shared component has `design/<name>.md`, `<name>.examples.ts` and a
  `<name>.workbench.ts` definition under Components/<Area>/<Label>. Examples use
  the shipping public API and meaningful states; docs end with Public API.
  Read `specs/ui/web-components.md` and `specs/ui/themes.md` before UI work.
- Keep code project-agnostic. Consuming projects receive configuration, not
  copied Workbench implementations. Update user guides when visible controls
  change, and the website overview/screenshots when those changes appear there.
- Use existing colocated Node tests, temporary Acme/example.com fixtures and
  meaningful behavioral assertions. Preserve public URLs and supported hosts.

## Commands and distribution

Run from the product root, or use `pnpm --dir packages/workbench` from the
repository root. The product coordinates one frozen pnpm lockfile with hoisted
Node dependencies and no dependency install scripts.

| Check or action | Command |
| --- | --- |
| Behavior across parts | `pnpm test` |
| Focused test files | `pnpm run test:focus <file> [--name pattern]` |
| Source extension / cached preparation | `pnpm run dev <project>` / `pnpm run dev:prepare` |
| TypeScript across source owners | `pnpm run check` |
| JavaScript/Web Component lint, formatting and unit/browser tests | `pnpm run check:ui` |
| UI lint / formatting / unit tests separately | `pnpm run lint`, `pnpm run format:check`, `pnpm run test:ui` |
| Rust format | `pnpm run format:server` |
| Rust format-check | `pnpm run format:check:server` |
| Rust lint | `pnpm run lint:server` |
| Rust types | `pnpm run types:server` |
| Rust test | `pnpm run test:server` |
| Rust doctest | `pnpm run doctest:server` |
| Rust docs | `pnpm run docs:server` |
| Rust deps | `pnpm run deps:server` |
| Rust coverage | `pnpm run coverage:server` (HTML under `server/target/llvm-cov/`) |
| Standalone server | `pnpm run serve <project>` |
| VSIX packaging | `pnpm run package` or `pnpm run package:all <target>` |
| Locked dependencies | `pnpm install --frozen-lockfile` |

Tests build the debug Rust executable. Packaging builds `workbench-server` with
the toolchain `server/rust-toolchain.toml` pins (Rspack's nightly) and locked
dependencies for the VSIX target, bundles esbuild for the Node evaluator and
the Electron runtime, and assembles a temporary product tree with the extension
manifest. Only `server/bin/<target>/` ships from Rust output; Cargo sources and
`target/` do not. Cross builds need the Rust target and linker or an explicit
`WORKBENCH_SERVER_BINARY`. Electron downloads are roughly 120–155 MB per target.
Cross-platform builds are unsigned and untested on the destination host.

The root product version, `extension/package.json` and `sdk/package.json` must
match. A `workbench/v<version>` release tag requires a matching CHANGELOG entry;
the workflow tests before packaging, publishes the SDK and distributes six
VSIX targets. Never push a tag without the user's go-ahead.

Run Rust verification categories individually and select only those needed for
the task. A request for tests does not also request formatting, lint, type
checking, doctests, API docs, dependency auditing or coverage. Name the selected
commands before running them; there is no aggregate Rust check target.
`docs` generates browsable HTML API reference in `target/doc/`; `doctest`
executes Rust documentation examples. Run either as its own explicit category.
Rustdoc can recheck dependencies even with `--no-deps`, which only suppresses
their generated reference pages. Keep Cargo caches between runs.

| Category | Command from `server/` |
| --- | --- |
| Apply formatting | `make format` |
| Verify formatting | `make format-check` |
| Strict Clippy | `make lint` |
| Type checking | `make types` |
| Tests | `make test` |
| Documentation examples | `make doctest` |
| API reference HTML | `make docs` |
| Dependency audit | `make deps` |

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
- Be conservative about running checks. Run scoped checks when they help, such
  as the tests covering changed code or a lint of the module being edited;
  Markdown-only changes need only a whitespace diff check. Do not run the full
  battery (whole suites, format, strict lint, dependency audits, coverage,
  builds, acceptance) after each change: when a task is complete, and before a
  commit, ask which relevant categories to run and name their individual commands.
  Command lists in package guides say how to run a check, not that every change
  must run it. Do not rerun a passing check unless code it covers changed since.
- Prefer the package's focused-test and development commands for iteration;
  preserve Cargo's `target/` cache and avoid release packaging or installation
  unless that is the behavior being verified.
- Select checks from the affected contract. Preserve the assertions and coverage
  scope while fixing failures; do not weaken thresholds, skip suites or broadly
  suppress rules to obtain a pass.
- Report executed commands and observed outcomes. Missing prerequisites, optional
  captures and platform limits are distinct from behavior failures and passing
  acceptance. Do not infer untested platform coverage from another host's result.

- Run Rust verification categories individually and select only those needed for
  the task. A request for tests does not also request formatting, lint, type
  checking, doctests, API docs, dependency auditing or coverage. Name the selected
  commands before running them; there is no aggregate Rust check target.
  `docs` generates browsable HTML API reference in `target/doc/`; `doctest`
  executes Rust documentation examples. Run either as its own explicit category.
  Rustdoc can recheck dependencies even with `--no-deps`, which only suppresses
  their generated reference pages. Keep Cargo caches between runs.

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

# MCP server instructions and tool descriptions

Agents choose tools from the server instructions and tool descriptions alone.
Hosts show the instructions before any tool is loaded, often while the tools
themselves are deferred.

- Server instructions take a few sentences: what the server is for, when an
  agent should reach for it (including what to do when it is blocked or stuck),
  and what it cannot do, such as authorizing actions or acting without the user.
- Each tool description is one short sentence on what the tool returns or
  changes. Name side effects and required user steps; leave argument details to
  the input schema.
- Use plain words an agent would search for. Avoid internal terms such as
  "normalized", "retained", "captured" or "owner-qualified" unless the schema
  defines them.
- Name only tools and commands that exist, and test that every tool named in the
  instructions is in the tool list.
- Shorten without dropping a trigger, a side effect or an authority limit.

---

# Modular architecture and incremental migration

These are the target architecture principles for Canonic packages. Package
guides describe current layouts, runtimes and compatibility constraints.

- Organize capabilities into modules with one clear owner for behavior, types,
  validation, state and lifecycle. Group files by responsibility; do not split
  every function into a file or create empty architectural layers.
- Place capability modules where the package's language expects them.
  JavaScript/TypeScript packages use `src/modules/<capability>/` for new
  capability modules and bounded capability migrations. Rust packages follow
  the Rust architecture rule. Keep the capability's logic, orchestration,
  dedicated adapters, types and tests together, following the package's test
  convention. A capability module represents an owned capability, not one file
  or every importable unit.
- Keep application composition, shared presentation, themes and host/platform
  infrastructure outside capability modules, in the locations defined by the
  package guide. Capability-specific adapters stay with their module;
  infrastructure shared across capabilities has its own explicit owner. Do not
  create empty folders or move code into a generic utilities directory to tidy
  the tree.
- Package guides document current locations, target locations and justified
  layout exceptions. Existing code outside the target location is not an
  instruction to move it all at once. Migrate within the requested capability,
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
- Match each package's language, module format, supported runtime, build and
  dependency constraints. Shared rules do not mandate a framework,
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

Specs hold internal requirements, workflows, system contracts, decisions,
discoveries, acceptance criteria, and known gaps. Docs hold reader-facing guides,
references, and troubleshooting. The package's writing rule defines their source
locations, audience, publishing, and validation.

- Read the relevant specification before changing product behavior. Update it
  when a requirement, decision, workflow, or material discovery changes. Separate
  agreed requirements, proposals, verified behavior, implementation gaps, and
  open questions. Existing code or passing tests do not establish product intent.
- Record durable discoveries with evidence and rationale. Label hypotheses;
  include dates and conditions for measurements where relevant. Keep current
  requirements prominent rather than accumulating a conversation transcript.
- Build experiments, probes and prototypes in temporary folders, not in the
  repository. Record their findings in the relevant spec, with enough detail
  about each fixture's setup to rebuild it, then delete the scripts. Product
  code and tests replace prototypes; they are not kept alongside them. The
  exception is a planned lab in a package whose labs spec allows committed
  labs; delete its runners once the decision ships.
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
- Check links and run `git diff --check`; run focused tests only when code also
  changes. Do not add tests that
  merely assert documentation text. Writing a spec does not authorize implementing
  proposals; writing docs does not itself authorize publishing or distribution.

---

# Rust package architecture

Rust packages apply the shared architecture principles with Rust's own
conventions. Where this rule and the language-neutral rule differ on layout,
this rule applies.

- Use Cargo's standard layout: `src/main.rs` for the binary, `src/lib.rs` for
  the library, `tests/` for integration tests, and `benches/` and `examples/`
  when needed. Keep one package until its size or reuse justifies a workspace;
  a workspace keeps its crates flat under `crates/`, with a virtual root manifest.
- Keep `main.rs` thin: parse arguments, set up configuration, call the
  library's `run`, and exit with its status. Output formatting and all other
  logic live in the library, where they can be tested.
- Product CLIs use `canonic-tui` as a required direct local dependency for
  terminal UI. Do not make it optional or gate it behind a Cargo feature.
  Runtime terminal capabilities and transport select interactive, plain or
  structured output; MCP, hooks and piped callers must not open interactive UI.
- Start every module with a `//!` comment saying what it owns.
- Build scripts write only inside `OUT_DIR`. Pass values to the code with
  `cargo:rustc-env` and read them with `env!` rather than writing files beside
  the build output.
- Put capability modules directly under `src/`, referred to as
  `crate::<capability>`. Do not add a `src/modules/` level. Use self-named
  module files (`src/ownership.rs`, or `src/ownership.rs` with `src/ownership/`
  for submodules) rather than `mod.rs`; enforce it with Clippy's
  `mod_module_files` lint.
- Layer dependencies inward. Domain modules hold pure rules and types with no
  process, filesystem or terminal I/O. Application modules carry out one use
  case each by coordinating domain and adapters. Adapter modules wrap external
  systems such as processes, the filesystem and the CLI. Domain code never
  depends on application or adapter code.
- Split application code by use case or command family, not into one file of
  every workflow.
- Keep visibility as narrow as the design allows. Use `pub(crate)` for items
  shared inside the package and `pub` only for the library's deliberate API.
  Test internals with unit tests in the module (`#[cfg(test)] mod tests`, or a
  `tests.rs` submodule once they outgrow the module); use `tests/` for behavior
  through the public interface or the built binary. Build integration tests as
  one binary, `tests/<name>/main.rs`, with a module per area and a shared
  fixture module, rather than one crate per file.
- Keep Clippy's `too_many_lines` on. A function that must stay long carries
  `#[expect(clippy::too_many_lines, reason = "...")]`.
- Add a trait only where a second implementation exists or a test needs to
  replace an external boundary that cannot run for real. Do not add traits only
  to mirror a layer diagram.
- Return typed errors (`thiserror` enums) where callers or tests branch on the
  kind of failure. Use `anyhow` with context in application code and at the
  binary boundary.
- Deny broken and private intra-doc links in `[lints.rustdoc]`. Generate API
  reference HTML through the separate `docs` target when that category is
  requested. Doctests are a separate `doctest` target. Never bundle Rust
  formatting, lint, types, tests, docs or dependency audits into an aggregate
  check command; select and run each category individually.
- Keep `Cargo.toml` `[lints]`, `rustfmt` and Clippy settings authoritative; the
  package guide lists the commands and configuration files.
- Run Cargo with the package guide's commands and Make targets, without extra
  flags, features, `RUSTFLAGS` or another target directory. Cargo never removes
  old build output, so each new combination adds another full set of artifacts
  to `target/`. Write logs, diffs and other outputs to a temporary directory,
  not `target/`. Do not run `cargo clean`: other sessions share `target/`, and
  cleaning forces their next builds and tests to start over.

---

# Rust test speed and determinism

Rust tests run in parallel, one process per test under nextest, and are
expected to be fast and repeatable. These rules apply the shared behavioral
testing rule to that setting.

- A test that passes alone but fails or slows sharply in the full suite points
  to a shared resource, a race or a blocking system call, not to "load". Find
  the cause before changing anything. Do not add retries, longer deadlines,
  serial-only execution or skips to make it pass.
- Keep the complete output of a failing or slow run in a temporary file and
  read the failure from it; do not filter output before the message is known.
  Reproduce intermittent failures on the focused set with
  `cargo nextest run --stress-count <n> --no-fail-fast` through the package's
  focused-test target, then verify a fix with more iterations than it took to
  reproduce.
- Measure before optimizing. Compare a test's time alone with its time in the
  suite: a large gap means contention, not slow code. Compare the sum of test
  times with wall time to tell a throughput-bound suite from one waiting on its
  longest test. For Git-heavy tests, `GIT_TRACE2_EVENT` separates Git's own
  time from the code around it. Changing the build profile is not a first step.
- Durable writes do not call `File::sync_all` or `sync_data` directly. On
  macOS they issue `F_FULLFSYNC`, which also empties the drive's write cache
  and waits behind every other process's flush, so parallel tests and frequent
  real operations serialize on it. Route flushes through one package helper
  that uses `fsync` on macOS and `sync_all` elsewhere, and keep the package's
  spec stating which flush its durable state receives.
- Tests never race work that outlives the command that started it. Wait for
  every child process a test or fixture starts. Git's automatic maintenance
  runs detached after commits and fetches and holds `objects/maintenance.lock`;
  packages whose tests run Git turn it off (`maintenance.auto = false`,
  `gc.auto = 0`) for every Git the tests start, including Git run by the code
  under test, and keep a test that fails when the setting does not apply.
- Tests do not depend on the developer's own configuration. Provide what tests
  need through the package's checked-in test environment, such as
  `.cargo/config.toml` `[env]` entries with `force = true` (so a developer's
  exported value does not replace them) pointing tools at test-owned
  configuration under `tests/`, rather than through a shell's environment or a
  user's dotfiles.
  Remember that code which scrubs variables such as `GIT_*` before starting a
  tool also drops test settings passed that way; choose a channel the code
  under test keeps.
- Wait on a condition with a deadline instead of sleeping for a fixed time.
  Deadlines bound a hang; they are not a synchronization mechanism, and a test
  that depends on one finishing in time is a race.
- Keep fixtures to the state an assertion needs and prefer an in-process call
  to starting a binary when the binary's own contract is not what is tested.
  Process-level tests stay for executables, exit statuses, output and process
  boundaries.

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
projects. The target contracts are `packages/workbench/specs/ui/web-components.md`
and `packages/workbench/specs/ui/themes.md`; read them before component or theme work.

- The UI is independent of the backend; the required contract is
  `packages/workbench/specs/system/modules.md#ui-and-backend-independence`. The backend
  supplies project data, operations and previewed content through APIs. It does
  not import UI runtime code, initialize components, or depend on their DOM or
  presentation state. Hosts load the UI and inject a data/operation
  client. UI-owned fixtures/tooling must support UI rendering without starting
  backend services; backend domain services must work without UI files. Generic
  UI asset delivery may share the API host and compiler libraries, but UI tooling
  remains independent of project compiler configuration, caches and lifecycle.
- Follow `packages/workbench/specs/system/rendering-flow.md`: one application shell per
  canvas document; artboards load dedicated content-only renderers. Never load
  the application entry point or bootstrap a shell inside an artboard, including
  a shell hidden with CSS. Project compilation/rendering is backend work;
  artboard composition is UI work. Preserve outer controls and warm content sessions.
  Verify child requests and document initialization before CSS suppression,
  alongside sizing, capture, disposal and independent instance behavior.

- Workbench UI files are browser-native JavaScript, HTML and CSS. No TypeScript,
  transpilation, bundling or generated implementation participates in UI delivery.
  Runtime imports, including shared browser rules, resolve directly to JavaScript.
- New components and substantial component migrations use autonomous Web
  Components in `ui/src/components/<name>/`. Use
  `wb-` tags, open Shadow DOM, owned styles, explicit inputs, and documented public
  events. Register at browser entry points; keep DOM modules out of Node imports.
- Controllers own application state and product operations. Elements own rendering
  and interaction, release connection resources on disconnect, and reconnect
  without duplicate listeners. Preserve focus/input on updates and never reach
  into another element's private DOM or rely on document-wide IDs.
- Components consume semantic `--wb-*` tokens. Shared theme assets under
  `ui/src/theme/` own complete standalone defaults and host mappings. VS Code colors
  are an adapter input, not a requirement for running a component.
- CSS variables cross Shadow DOM boundaries, not iframe documents. The embedding
  webview owns validated theme forwarding, initial readiness, live updates and
  snapshot replacement. Keep themes document-scoped and out of project previews,
  captures, configuration and server state.
- Verify native semantics, keyboard and focus in a real browser; cover defaults,
  custom host colors and high contrast. Check direct webview and independent browser-host asset
  loading, CSP, and packaging for migrated components and styles.
- Every element in `ui/src/components/<name>/` has its docs page in the Workbench
  space, in the same change: `design/<name>.md` with one section per meaningful
  state and a **Public API** section, `design/<name>.examples.ts` rendering the
  shipping element through its public inputs, and `design/<name>.workbench.ts`
  (`defineDocs`, `components/<name>`, `Components/<Area>/<Label>`, grouped by the interface area it appears in). A changed API or
  visible state updates the page with it. Examples import only what they draw.
  The contract is `specs/ui/web-components.md#design-pages`; the
  `design-pages.test.ts` check fails a component without one.
- Existing vanilla UI is migration debt, not evidence that the target is shipped.
  Local fixes may remain in place; migrate bounded controls when substantially
  reworking them. Do not combine a component task with an unsolicited full UI
  rewrite, a new rendering framework, or changes to project preview adapters.

These instructions refine the shared UI contracts;
they do not distribute a UI package or configuration to consuming projects.

---

# Workbench writing locations and checks

- Internal specs: `packages/workbench/specs/`, indexed by `specs/README.md`.
  Root topics own the product model and vocabulary. `canvas/`, `pages/`, and
  `review/` group capability contracts; `ui/` owns shared presentation and
  `integrations/` owns editor/agent hosting. `system/` owns architecture,
  performance budgets, quality gates and lab method. `preview-pipeline/` and
  `adapters/` own shared rendering mechanics and technology differences.
  `research/` owns dated observations and evidence; it does not redefine
  contracts. Keep folders one level deep and maintain their ownership indexes.
  Update incoming links when moving topics, including guide/skill sources,
  source comments and lab records; regenerate agent instructions as needed.
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
- Markdown changes need link review and `git diff --check`. When the user asks
  for checks, public guide changes run `pnpm --dir packages/website test` and
  `pnpm --dir packages/website run build`, then inspect affected pages, and
  Workbench behavior changes run `pnpm --dir packages/workbench test`.
- When visible behavior affects overview copy or screenshots, update the
  website's Workbench overview and screenshots. Consuming projects supply
  configuration, not copies of Workbench scripts. Publishing follows the separately authorized release workflow.
