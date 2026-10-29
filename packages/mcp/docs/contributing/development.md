# Development

This page covers contributing to the library itself. Run the commands below
from `packages/mcp`.

This is one unpublished Cargo package with a library and no binary. It uses
Rust 1.99.0, Rust 2024, a committed `Cargo.lock`, rustfmt, strict Clippy,
nextest, doctests, rustdoc and cargo-deny.

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
`~/.cargo/bin/cargo` so rustup selects the pinned toolchain, falling back to
Cargo on PATH. `CARGO` overrides that selection. Install the tools with:

```sh
cargo install --locked cargo-nextest@0.9.146 cargo-deny@0.20.2 cargo-llvm-cov@0.9.1
```

## Fast iteration

```sh
make dev
make test-focus FILTER='session::'
# Integration tests:
make test-focus FILTER='scheduling::' TEST_TARGET='--test integration'
# Repeat the integration tests to expose races:
make test-focus FILTER='::' TEST_TARGET='--test integration' CARGO_FLAGS='--stress-count 30 --no-fail-fast'
```

`test-focus` requires a substring filter and defaults to `--lib`. Keep this
package's `target/` directory between runs.

## Configuration

- `Cargo.toml` owns lint policy: `unsafe_code` is forbidden, `unreachable_pub`
  warns, Clippy pedantic and `unwrap_used` warn (errors under `-D warnings`),
  `mod_module_files` enforces self-named modules, and rustdoc denies broken
  and private links.
- `clippy.toml` allows `unwrap` in tests only.
- `deny.toml` allows Apache-2.0, MIT and Unicode-3.0.
  A crate-scoped Zlib exception covers `foldhash`, used by the YAML parser's
  hash tables.
- `.config/nextest.toml` disables fail-fast and terminates hung tests.

## Tests

Unit tests sit in each module, or in its `tests.rs` submodule. The integration
binary, `tests/integration/main.rs`, runs `stdio::serve` on a thread with
in-memory pipes and a scriptable handler; it opens no sockets or processes.
Wait on recorded events with the fixture's bounded wait rather than sleeping,
and keep deadlines short only to make expiry happen.

See the [documentation index](../README.md) for usage guides. When changing
an API, keep its rustdoc and the corresponding guide consistent.

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
