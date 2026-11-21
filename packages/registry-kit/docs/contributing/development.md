# Development

For application integration tests, start with
[Test your integration](../guides/testing.md). This page covers contributing to
the library itself. Run the commands below from `packages/registry-kit`.

This is one unpublished Cargo package with a library and no binary. It uses
Rust nightly-2026-11-09 (rustc 1.102), Rust 2024, a committed `Cargo.lock`, rustfmt, strict Clippy,
nextest, doctests, rustdoc, cargo-deny, insta and proptest.

| Task | Command (from this directory) |
| --- | --- |
| Formatting | `cargo fmt --check` |
| Lint | `cargo clippy --locked --all-targets --all-features` |
| Tests | `cargo nextest run --locked --all-features` |
| Doctests | `cargo test --doc --locked --all-features` |
| API docs | `cargo doc --locked --all-features --no-deps --document-private-items` |
| Dependencies | `cargo deny check` |
| Coverage report | `cargo llvm-cov nextest --locked --all-features --html` (writes `target/llvm-cov/`) |
| Release build | `cargo build --release --locked --all-features` |
| Free disk space | `cargo sweep --time 3` (from cargo-sweep), while no build or test uses `target/` |

Add `--offline` to use cached dependencies. Run Cargo from this directory so
rustup selects the pinned toolchain. Install the tools with:

```sh
cargo install --locked cargo-nextest@0.9.146 cargo-deny@0.20.2 cargo-llvm-cov@0.9.1 cargo-insta@1.49.0
```

## Fast iteration

From this package directory:

```sh
cargo build --locked --all-features
cargo nextest run --locked --all-features --lib 'digest::'
# Select integration tests explicitly when needed:
cargo nextest run --locked --all-features --test integration 'selected_test_name'
```

`cargo build` uses Cargo's incremental development build.
It does not install a command or build a release. Pass `--lib` with a
substring filter to avoid unrelated integration and binary test targets.
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
  and `unwrap_used` warn (`.cargo/config.toml` denies warnings), `mod_module_files`
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
the task. A request for tests does not also request formatting, lint,
doctests, API docs, dependency auditing or coverage. Clippy also type-checks
every target. Name the selected commands before running them; there is no
aggregate Rust check command. `cargo doc` generates browsable HTML API
reference in `target/doc/`; `cargo test --doc` executes Rust documentation
examples. Run either as its own explicit category.
Rustdoc can recheck dependencies even with `--no-deps`, which only suppresses
their generated reference pages. Keep Cargo caches between runs.

| Category | Command from this package |
| --- | --- |
| Apply formatting | `cargo fmt` |
| Verify formatting | `cargo fmt --check` |
| Strict Clippy and type checking | `cargo clippy --locked --all-targets --all-features` |
| Tests | `cargo nextest run --locked --all-features` |
| Documentation examples | `cargo test --doc --locked --all-features` |
| API reference HTML | `cargo doc --locked --all-features --no-deps --document-private-items` |
| Dependency audit | `cargo deny check` |
