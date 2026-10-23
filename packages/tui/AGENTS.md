# Rust TUI — Package Instructions

Read README.md and specs/README.md before changing components. This unpublished
library supplies reusable views to consuming CLIs through local path
dependencies; its optional gallery feature builds tui-gallery. No root Cargo
workspace or publication pipeline exists.

## Ownership and boundaries

- picker.rs and chat.rs own component state, input-to-action translation and
  rendering. Hosts own approvals, agent connections, policy and Git operations.
- theme.rs owns semantic styling; text.rs owns bounded sanitized display text and
  grapheme wrapping. runtime.rs owns one terminal lease and restores modes on drop.
- gallery.rs composes the public components with deterministic fixtures; main.rs
  only parses options and reports errors. It never calls agents or product CLIs.
- Components do not read terminal input, enter raw mode or start event loops.
  One application owns the terminal and dispatches events. Preserve plain/piped
  product output; gallery --list and --help work without a terminal.
- Rust modules are self-named files directly under src/. Shared runtime and themes
  stay outside component modules. Public contracts have docs and deliberate APIs.

## Development and checks

Rust 1.99.0, Rust 2024 and Cargo.lock are pinned.
make -C packages/tui run opens the selector. make build builds the release
gallery. Select individual Make targets;
make coverage runs cargo llvm-cov nextest. Use CARGO_FLAGS=--offline only for
cached dependencies. Quality tooling and artifact locations are in docs/development.md.

For fast iteration, use `make dev` and `make test-focus FILTER='module::'`
from this package; focused tests default to the component integration binary.
Keep `target/` to reuse Cargo's cache; `TEST_TARGET` selects other test targets.

Run these from packages/tui:

```sh
cargo fmt --check
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo nextest run --locked --all-features
cargo test --doc --locked --all-features
cargo doc --locked --all-features --no-deps --document-private-items
cargo deny check
cargo build --release --locked --all-features
```

Tests use one integration binary at tests/integration/main.rs. Test state/actions,
public rendering through TestBackend, reviewed insta snapshots, proptest
invariants and real PTY restoration/navigation/resize. PTY tests require Python 3
on Unix and host access when run in an agent sandbox. Run process-heavy suites
separately. Test rendered behavior rather than repeating documentation or internal
structure. Keep fixtures temporary and preserve user terminals and processes.
Use narrow justified lint expectations only; keep too_many_lines enabled.

Run git diff --check in each affected repository. Publication remains disabled.

Run Rust verification categories individually and select only those needed for
the task. A request for tests does not also request formatting, lint, type
checking, doctests, API docs, dependency auditing or coverage. Name the selected
commands before running them; there is no aggregate Rust check target.
`docs` generates browsable HTML API reference in `target/doc/`; `doctest`
executes Rust documentation examples. Run either as its own explicit category.
Rustdoc can recheck dependencies even with `--no-deps`, which only suppresses
their generated reference pages. Keep Cargo caches between runs.

| Category | Command from this package |
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

# Rust TUI writing locations and checks

- Specs for architecture, runtime ownership, components and acceptance live
  in packages/tui/specs/, indexed by README.md.
- Guides for supported usage and development live in packages/tui/docs/,
  indexed by README.md and the package README.
- This is an unpublished local Rust library and component gallery. No release
  registry or publishing integration exists. Keep fixtures independent of products,
  accounts and operations; use Acme/example.com examples.
- Document actions returned to hosts, input/focus ownership, cancellation, rendering
  limits and headless behavior. A component never grants product authorization.
- Verify Markdown links and each affected repository's diff. Regenerate
  agent outputs after changing guides or configuration.
