# Development and checks

Agent Kit is one unpublished Rust library, with Rust 2024, the pinned
nightly-2026-04-16 toolchain (Rust 1.97), and a committed Cargo.lock. Run these
commands from the repository root. Select verification categories individually.

| Task | Command |
| --- | --- |
| Incremental build | `make -C packages/agent-kit dev` |
| Release build | `make -C packages/agent-kit build` |
| Apply formatting | `make -C packages/agent-kit format` |
| Check formatting | `make -C packages/agent-kit format-check` |
| Type checking | `make -C packages/agent-kit types` |
| Strict Clippy | `make -C packages/agent-kit lint` |
| Tests | `make -C packages/agent-kit test` |
| Focused tests | `make -C packages/agent-kit test-focus FILTER=ownership_conflicts` |
| Documentation examples | `make -C packages/agent-kit doctest` |
| API reference HTML | `make -C packages/agent-kit docs` |
| Dependency audit | `make -C packages/agent-kit deps` |
| Coverage HTML | `make -C packages/agent-kit coverage` |

Install cargo-nextest, cargo-deny and cargo-llvm-cov for tests, dependency
auditing and coverage respectively. Rustfmt, Clippy and the LLVM tools needed
for coverage are pinned toolchain components.
The repository's documented tool versions can be installed from this package:

```sh
cargo install --locked cargo-nextest@0.9.146 cargo-deny@0.20.2 cargo-llvm-cov@0.9.1
```

`CARGO` overrides the Cargo executable; `CARGO_FLAGS=--offline` uses cached
dependencies. Preserve `target/` between runs. API docs go to `target/doc/` and
coverage to `target/llvm-cov/`.

Tests run under nextest, one process per test, with fail-fast disabled and a
bounded slow-test timeout in `.config/nextest.toml`. `test-focus` requires a
test-name substring and defaults to library tests; `TEST_TARGET` can select a
future integration target. No additional test framework is needed for the
current fixtures.

`src/tests.rs` exercises registration, discovery, host encodings, formatting
preservation, tool approvals, ownership conflicts, repair/removal, and partial
publication. `storage.rs` has a kernel-lock lifetime regression. Fixtures own
temporary project, home and state roots. Unix cases exercise permissions,
symlinks, FIFOs and trusted catalog-check child processes; use host access when
the sandbox restricts those operations. These fixtures do not establish live
agent connection or terminal UI acceptance, or cross-platform verification.

Cargo.toml forbids unsafe code, warns on unreachable public items, denies
broken/private rustdoc links, and enables Clippy's pedantic, unwrap and
self-named module rules. `make lint` treats warnings as errors. `clippy.toml`
permits unwrap in tests; production code returns errors. `deny.toml` checks
dependency advisories, sources and licenses, with crate-scoped exceptions for
foldhash (Zlib), option-ext 0.2.0
(MPL-2.0), and encoding_rs (BSD-3-Clause), matching existing consumer policies.

[Usage and ownership](README.md)
