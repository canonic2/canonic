# Code quality

Status: implemented engineering baseline for Workbench's Rust page index and
its Node integration. Passing these checks is distinct from meeting the
[performance targets](preview-pipeline/performance.md#measured-interactions)
or accepting the compiler-host prototype.

## Scope and ownership

This topic owns tooling, static checks, test conventions and dependency policy.
Feature acceptance belongs to the relevant spec, including
[page-index acceptance](preview-pipeline/page-index.md#acceptance-and-evidence).
Commands and pinned tool-installation instructions live in the
[native README](../server/README.md#checks-and-packaging) and the
[package guide](../AGENTS.md#commands-and-distribution).

The baseline follows the repository's Rust packages, including TUI and
Registry. Workbench keeps its own configuration and lockfile; it does not
introduce a root Cargo workspace or couple product releases.

## Required gates

| Area | Contract |
| --- | --- |
| Toolchain | Pin stable Rust 1.99.0 with rustfmt, Clippy and LLVM tools; commit `Cargo.lock` |
| Formatting | Use rustfmt defaults |
| Lint | Check every target and feature with Clippy and `-D warnings` |
| Rust policy | Forbid unsafe code; warn on unreachable public items, Clippy pedantic, production unwraps and `mod_module_files`; deny broken and private rustdoc links |
| Tests | Run behavioral tests with nextest and doctests separately; use locked dependencies and all features |
| Documentation | Build dependency-free API documentation, including private items |
| Dependencies | Check advisories, licenses, versions and sources with cargo-deny |
| Coverage | Generate an HTML report with cargo-llvm-cov over nextest; no minimum percentage gate |
| Shipping | Build the target's native release executable with locked dependencies before packaging |
| Host integration | Run the Workbench Node suite and TypeScript check alongside the native gates |

`make -C server check` runs the Rust gates. The package-level
`check:server` command delegates to it, and the extension release workflow
runs it before packaging. Coverage is a separate developer command. CI installs
the same pinned nextest and cargo-deny tools documented for local development.

Clippy lint levels belong in `Cargo.toml`; test-only unwrap permission belongs
in `clippy.toml`. Do not suppress broad lint groups or disable
`too_many_lines` to make a gate pass. Legitimate local exceptions require a
specific rationale.

## Behavioral evidence

- Unit tests cover literal extraction, definition validation/projection and
  saved-index behavior. Proptest checks nested/escaped literals and rejection
  of executable values from the literal path.
- Insta records stable page-list JSON alongside explicit behavior assertions.
  Review intended snapshot changes; accepting a snapshot is not verification
  that its behavior is correct.
- Keep one native integration binary at `tests/integration/main.rs`.
  Assert_cmd, assert_fs and predicates exercise the real CLI with temporary
  project and saved-index resources that are cleaned up together.
- Node adapter tests compare the Rust answer against the compiler across
  discovery, evaluation, ordering, validation and failure cases. Server tests
  exercise trust and disabled-preview boundaries.
- Nextest reports every failure rather than stopping at the first. Flag a
  slow test after 30 seconds and terminate it after two minutes.
- Run process-heavy suites separately. Sandbox access must support child
  processes and signals, Node loopback fixtures, and cargo-deny's advisory
  cache; a sandbox refusal is an unavailable prerequisite, not a product
  failure or a reason to weaken the check.

Source-level tests, a passing host run, all-platform installation, webview
acceptance and performance measurements are separate evidence. Record missing
behavioral evidence in the owning feature spec rather than treating tooling
parity as completed acceptance.

## Dependency and artifact policy

Allow Apache-2.0, MIT and Unicode-3.0 licenses. Ignore the unpublished package's
own license, warn on duplicate versions, deny wildcard versions and unknown
Git/registry sources, and check advisories. Package-specific license
exceptions permit `Apache-2.0 WITH LLVM-exception` for `ar_archive_writer`, used
by SWC's stacker/psm build dependencies; `CC0-1.0` for `notify` and `ISC` for
`inotify` and `inotify-sys`, the file watcher and its Linux backend; and
`BSD-3-Clause` for `matchit`, the HTTP router's path matcher.

Runtime dependencies retain exact manifest pins to align parser inputs with
the planned compiler-host evaluation. Test dependencies use the same tools
and feature settings as the peer Rust packages. Changing a dependency or adding
an exception requires reviewing the resulting lockfile and dependency checks.

Builds, coverage reports and staged platform binaries remain under ignored
`server/target/` and `server/bin/`. Only the selected platform executable enters
the VSIX. Development tooling, source and reports do not ship in it.
