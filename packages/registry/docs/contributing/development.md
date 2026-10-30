# Development

For application integration tests, start with
[Test your integration](../guides/testing.md). This page covers contributing to
the library itself. Run the commands below from `packages/registry`.

This is one unpublished Cargo package with a library and no binary. It uses
Rust 1.99.0, Rust 2024, a committed `Cargo.lock`, rustfmt, strict Clippy,
nextest, doctests, rustdoc, cargo-deny, insta and proptest.

| Task | Command (from this directory) |
| --- | --- |
| Formatting | `cargo fmt --check` |
| Lint | `cargo clippy --locked --all-targets --all-features -- -D warnings` |
| Tests | `cargo nextest run --locked --all-features` |
| Doctests | `cargo test --doc --locked --all-features` |
| API docs | `cargo doc --locked --all-features --no-deps --document-private-items` |
| Dependencies | `cargo deny check` |
| Coverage report | `make coverage` (writes `target/llvm-cov/`) |
| Release build | `make build` |

`CARGO_FLAGS=--offline` uses cached dependencies. Make prefers
`~/.cargo/bin/cargo` so rustup selects the pinned toolchain, falling back to Cargo
on PATH. `CARGO` overrides that selection. Install the tools with:

```sh
cargo install --locked cargo-nextest@0.9.146 cargo-deny@0.20.2 cargo-llvm-cov@0.9.1 cargo-insta@1.49.0
```

## Fast iteration

From this package directory:

```sh
make dev
make test-focus FILTER='digest::'
# Select integration tests explicitly when needed:
make test-focus FILTER='selected_test_name' TEST_TARGET='--test integration'
```

`dev` uses Cargo's incremental development build.
It does not install a command or build a release. `test-focus` requires a
substring filter and defaults to `--lib`, avoiding unrelated integration and
binary test targets. It uses the existing nextest runner.
Use a real test name in the integration example; nextest fails when nothing matches.
Keep this package's `target/` directory between iterations; Cargo checks source
freshness on every run. Dependency, toolchain, profile and feature changes may
require a longer rebuild. Use release builds
for performance measurements. Select completion/release verification categories
individually. Existing build, installation and acceptance commands retain their
behavior.

## Configuration

- `Cargo.toml` owns lint policy: `unsafe_code` is forbidden, `unreachable_pub`
  warns, Clippy pedantic
  and `unwrap_used` warn (errors under `-D warnings`), `mod_module_files`
  enforces self-named modules, and rustdoc denies broken and private links.
- `clippy.toml` allows `unwrap` in tests only.
- `deny.toml` allows Apache-2.0, MIT and Unicode-3.0, plus reviewed
  exceptions for Rustls and ring components and webpki roots.
- `.config/nextest.toml` disables fail-fast and terminates hung tests.

## Tests

Unit tests sit in each module, or in its `tests.rs` submodule. The integration binary,
`tests/integration/main.rs`, needs `--all-features` (or
`--features test-support`) and opens loopback TLS listeners.

The lockfile snapshot is reviewed with insta: after an intended change, run
`cargo insta test --all-features --review` (or `--accept` once reviewed).
Source identities and the one artifact that embeds an identity vary per run,
so the snapshot redacts them.

The TLS fixtures in `src/testing/fixtures/` are public test data valid until
2126. They must never be used for real services.

See the [documentation index](../README.md) for public usage guides. When changing
an API, keep its rustdoc and the corresponding usage guide consistent.

[Documentation index](../README.md)

## Individual Rust verification

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
