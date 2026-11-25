# Development and checks

Agent Kit is one unpublished Rust library, with Rust 2024, the pinned
nightly-2026-11-09 toolchain (Rust 1.102), and a committed Cargo.lock. Run these
commands from `packages/agent-kit/`, where rustup selects the pinned toolchain.
Select verification categories individually.

| Task | Command |
| --- | --- |
| Incremental build | `cargo build --locked --all-features` |
| Release build | `cargo build --release --locked --all-features` |
| Apply formatting | `cargo fmt` |
| Check formatting | `cargo fmt --check` |
| Strict Clippy and type checking | `cargo clippy --locked --all-targets --all-features` |
| Tests | `cargo nextest run --locked --all-features` |
| Focused tests | `cargo nextest run --locked --all-features --lib ownership_conflicts` |
| Documentation examples | `cargo test --doc --locked --all-features` |
| API reference HTML | `cargo doc --locked --all-features --no-deps --document-private-items` |
| Dependency audit | `cargo deny check` |
| Coverage HTML | `cargo llvm-cov nextest --locked --all-features --html` |
| Free disk space | `cargo sweep --time 3`, while no build or test uses `target/` |
| Resolve the lockfile from the local cache | `cargo generate-lockfile --offline` |

Install cargo-nextest, cargo-deny and cargo-llvm-cov for tests, dependency
auditing and coverage respectively. Rustfmt, Clippy and the LLVM tools needed
for coverage are pinned toolchain components.
The repository's documented tool versions can be installed from this package:

```sh
cargo install --locked cargo-nextest@0.9.146 cargo-deny@0.20.2 cargo-llvm-cov@0.9.1
```

`cargo install --locked cargo-sweep` adds `cargo sweep`. Add `--offline` to a
command to use cached dependencies. Preserve `target/` between runs. API docs go to `target/doc/` and
coverage to `target/llvm-cov/`.

Tests run under nextest, one process per test, with fail-fast disabled and a
bounded slow-test timeout in `.config/nextest.toml`. A focused run takes a
test-name substring; `--lib` selects library tests, and `--test <name>` would
select a future integration target. No additional test framework is needed for the
current fixtures.

Use the package's unoptimized development profile for routine tests. Cargo's
test profile inherits its optimization settings. Optimizing dependencies
adds substantial compile time on a fresh target for a small saving on a warm
test run. When investigating a slow test, compare its time in the full suite
with a focused run before profiling it. Assess any profile change using
warm test runs, incremental rebuilds and fresh-target builds.

`src/tests.rs` exercises registration, discovery, host encodings, formatting
preservation, tool approvals, ownership conflicts, repair/removal, and partial
publication. `storage.rs` has a kernel-lock lifetime regression. Fixtures own
temporary project, home and state roots. Unix cases exercise permissions,
symlinks, FIFOs and trusted catalog-check child processes; use host access when
the sandbox restricts those operations. These fixtures do not establish live
agent connection or terminal UI acceptance, or cross-platform verification.

Cargo.toml forbids unsafe code, warns on unreachable public items, denies
broken/private rustdoc links, and enables Clippy's pedantic, unwrap and
self-named module rules. `.cargo/config.toml` denies warnings, so Cargo fails on them. `clippy.toml`
permits unwrap in tests; production code returns errors. `deny.toml` checks
dependency advisories, sources and licenses, with crate-scoped exceptions for
foldhash (Zlib), option-ext 0.2.0
(MPL-2.0), and encoding_rs (BSD-3-Clause), matching existing consumer policies.

[Usage and ownership](README.md)
