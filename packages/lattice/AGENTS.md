# Lattice — Package Instructions

Lattice is an unpublished native JavaScript package with an empty ESM entry
point and development tooling. Use README.md for setup and command details.

- Capability code belongs in src/modules/<capability>/; src/index.js owns the
  public package surface. Runtime source is served unchanged.
- Tests beside modules use *.test.js or *.browser.test.js. Package/tooling tests
  live in tests/. Keep dependencies and the pnpm lockfile local to this package.
- Do not migrate consumers or implement framework APIs as part of a tooling
  change. Preserve unrelated local work.

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
- Do not run checks on your own, including focused tests, lint, type checks,
  formatting checks, dependency audits, coverage, verification builds,
  acceptance suites and whitespace diff checks. Run checks only when the user
  explicitly requests them. Checklists in package guides are reference, not
  permission to run checks. Do not ask which
  checks to run after a task or before a commit; report existing results and
  unverified areas. Do not rerun a passing check unless code it covers changed since.
  While debugging a reported failure, running the failing test or reproduction
  command, and rerunning it after the fix, needs no request.
- Prefer the package's focused-test and development commands for iteration;
  preserve Cargo's `target/` cache and avoid release packaging or installation
  unless that is the behavior being verified.
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

# Lattice development

## JavaScript and documentation

- Write modern native JavaScript in `.js` files, including tests and executable
  configuration. Use `import`/`export` and explicit relative file extensions.
  CommonJS needs a documented host compatibility requirement.
- Require meaningful JSDoc on every module, named function, class, method and
  private helper, including named arrow functions. Document parameter and return
  types, structured values, events, generics and lifecycle obligations. Inline
  callbacks may inherit types from the documented API that accepts them.
- Keep strict JavaScript/JSDoc type checking with `allowJs`, `checkJs`, `strict`
  and `noEmit`. Do not hide incomplete contracts behind broad `any`, blanket
  suppression or unchecked casts. TypeScript checks source; it does not emit it.
- Use named JSDoc object/callback types and type imports for shared contracts.
  Public API documentation covers timing, equality, errors, cancellation and
  cleanup where relevant. Empty comments or prose repeating a name are not enough;
  static checking does not establish nested immutability or runtime validity.

## Delivery and modularity

- Ship source unchanged, with zero runtime dependencies. Add no production
  build, transpilation, runtime CDN imports or dependency on development tools.
- Honor `sideEffects: false` in every runtime module. Imports must not register
  elements, touch the DOM, mutate host globals, install listeners, start timers
  or perform I/O. Initialization happens through explicit calls.
- Keep exports statically analyzable. Expose deliberate public entry points and
  explicit capability subpaths when needed, without wildcard internal exports.
  Keep pure state imports independent of DOM integrations and optional features.
- Keep import graphs narrow and acyclic. Do not route internal imports through
  an aggregate entry point that pulls in unrelated capabilities. Native browsers
  load reachable modules without tree shaking. A bundle-elimination claim needs
  consumer bundle evidence; ESM metadata alone does not establish it.
- Keep syntax and APIs within the minimum runtimes of each entry point. Pure
  modules use shared ECMAScript and platform APIs, without DOM or Node-only
  globals; DOM modules use browser APIs and no Node API. Check their environments
  separately with appropriately scoped types. A passing type check does not
  establish that a minimum runtime actually provides an API.

## Ownership and updates

- Put capability code under `src/modules/<capability>/`. Keep consumer routing,
  persistence, transport, application stores and visual design outside Lattice.
  Framework mechanics must not special-case a consuming product.
- Keep dedicated adapters, types and tests with their capability. Shared
  framework infrastructure may live directly under `src/` with documented
  ownership. Add no empty module or directory just to match a writing topic.
- Preserve state → binding → element separation. Pure state has no DOM or I/O.
  Bindings connect selected state to inputs and events to actions; they do not
  decide product rules. Elements work with plain inputs and events without stores.
- Give each mutable fact one owner and derive dependent values. Use explicit
  ports for external operations, not global singletons or service locators.
- Keep unchanged writes silent and preserve unchanged object identities. Update
  only affected nodes and properties. Retain keyed nodes and cached references;
  do not rebuild subtrees or icons for unrelated input changes. Preserve focus,
  input, selection and scroll.
- Avoid repeated full-list traversal, cloning and serialization in hot paths.
  Separate geometry reads from DOM writes to avoid forced layout. Introduce
  scheduling only for a defined need without delaying immediate interaction.
- Support performance claims with notification, selector, allocation, traversal
  or DOM-write counts, or timings from a named environment. Tool speed does not
  establish runtime speed; small code alone does not establish efficiency.

## Lifetimes and verification

- Own every listener, subscription, observer, timer, worker and pending operation
  through explicit cleanup or an owning `AbortSignal`. Handle already-aborted
  signals before acquisition and clean up partial initialization failures.
- Make cleanup idempotent. Prevent stale callbacks from changing state or DOM
  after disposal or supersession. Cancellation does not replace operation-identity
  checks. Observe rejected promises and document how outstanding work settles.
  Reconnection must not duplicate resources. Keep handles out of state snapshots.
- Mark lifetimes ended before releasing resources and attempt all releases in
  reverse acquisition order even if one fails. Preserve an original setup/action
  error; route cleanup errors through the owner's error policy. Standalone cleanup
  attempts every step before throwing or aggregating failures; abort-triggered
  cleanup reports them instead. Repeated disposal does not retry failed releases.
- Use Oxlint and Oxfmt, strict JSDoc type checking, and Vitest with Playwright.
  Keep tools as pinned development dependencies. Pure behavior runs in Node;
  Web Component, DOM and browser lifecycle behavior uses real Chromium.
- Keep tests beside capabilities as `*.test.js` and `*.browser.test.js`, with
  package/tooling fixtures under `tests/`. Use the package-local pnpm lockfile
  and independent commands from the README; add no aggregate verification flow.
- Add behavioral coverage for changed contracts, failure, cleanup and late
  completion where applicable. Run checks only when explicitly requested;
  the package README lists independent commands.

---

# Lattice writing locations and checks

- Keep internal contracts separate from reader-facing API guides. Update only
  the topic affected by the requested writing change.
- Separate ordinary authoring examples, source architecture and detailed
  guarantees. Keep each detailed requirement with one owner and link to it instead
  of duplicating its rules. Do not choose an authoring format or implementation
  design as part of restructuring documents. Keep a README in reserved folders.
- The package README owns setup and development commands. Add reader-facing
  API guides under packages/lattice/docs/ when supported APIs exist, with a
  README index. Do not describe proposed APIs as available.
- Lattice is an unpublished, product-agnostic native JavaScript package. Use
  generic consumers and Acme/example.com fixtures. No website publishing or
  application migration is implied by a documentation change.
- Review links and affected repository diffs. Regenerate agent instructions
  after changing package guides, writing rules or registration.

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
- Verification specs map each contract to the tests that exercise it and list
  the gaps. They do not log check runs: no pass records, run dates, product or
  tool versions of a run, test or link counts, or coverage results. Report run
  results to the user, not in specs or docs.
- Build experiments, probes and prototypes in temporary folders, and labs in
  the package's ignored `labs/` folder; none of them is committed, and no
  committed file links into `labs/`. Product code and tests replace
  prototypes; they are not kept alongside them.
- Each lab ends as one research record, `research/<topic>.md` at the package's
  top level, named for its question and listed with its status in
  `research/README.md`. Its sections, in order: Question, Status, Hypothesis,
  Method, Conditions, Results, Correctness, Threats to validity, and Decision
  and follow-ups, with enough fixture and variant detail to rebuild the lab.
  Follow-ups add dated subsections to the same record. An optional
  `research/<topic>.json` holds small summaries only; raw samples, logs,
  archives and runner code stay in `labs/`. Reusable fixture generators are
  committed under `fixtures/labs/`. Research uses neutral paths and host
  details: no usernames, home or temporary paths, hostnames or device IDs.
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
- Run documentation checks, `git diff --check` and focused tests only when the
  user explicitly requests them. Do not add tests that
  merely assert documentation text. Writing a spec does not authorize implementing
  proposals; writing docs does not itself authorize publishing or distribution.
