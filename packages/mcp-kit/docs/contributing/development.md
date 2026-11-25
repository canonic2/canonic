# Development

This page covers contributing to the library itself. Run the commands below
from `packages/mcp-kit`.

This is one unpublished Cargo library package with a test-only worker fixture
executable and no product CLI. It uses
Rust nightly-2026-11-09 (rustc 1.102), Rust 2024, a committed `Cargo.lock`, rustfmt, strict Clippy,
nextest, doctests, rustdoc and cargo-deny.

| Task | Command (from this directory) |
| --- | --- |
| Formatting | `cargo fmt --check` |
| Lint | `cargo clippy --locked --all-targets --all-features` |
| Tests | `cargo nextest run --locked --all-features` |
| Doctests | `cargo test --doc --locked --all-features` |
| API docs | `cargo doc --locked --all-features --no-deps --document-private-items` |
| Dependencies | `cargo deny check` |
| Coverage report | `cargo llvm-cov nextest --locked --all-features --html` (writes `target/llvm-cov/`) |
| Apps example's inline script and host bridge | `node --test examples/apps/status.test.mjs` (Node built-in tests; no browser or network) |
| Release build | `cargo build --release --locked --all-features` |
| Free disk space | `cargo sweep --time 3` (from cargo-sweep), while no build or test uses `target/` |

Add `--offline` to use cached dependencies. Run Cargo from this directory so
rustup selects the pinned toolchain. Install the tools with:

```sh
cargo install --locked cargo-nextest@0.9.146 cargo-deny@0.20.2 cargo-llvm-cov@0.9.1
```

## Fast iteration

```sh
cargo build --locked --all-features
# Ordinary build without optional driver/dependency features:
cargo build --locked
cargo nextest run --locked --all-features --lib 'session::'
# Build the runnable Tasks example with the existing development profile:
cargo build --locked --all-features --example tasks
# Integration tests:
cargo nextest run --locked --all-features --test integration 'scheduling::'
# Process/HTTP replacement fixtures require host access in a sandbox:
cargo nextest run --locked --all-features --test integration worker_replacement
# Repeat the integration tests to expose races:
cargo nextest run --locked --all-features --test integration --stress-count 30 --no-fail-fast '::'
```

Pass `--lib` or `--test integration` with a substring filter to run focused
tests. Keep this package's `target/` directory between runs. To free space, run
`cargo sweep --time 3` on its own while no build or test uses `target/`; it
deletes build output unused for more than three days.

After intentionally changing dependencies, `cargo generate-lockfile` resolves the lockfile.
Inspect its diff and preserve unrelated locked versions.

## Configuration

- `Cargo.toml` owns lint policy: `unsafe_code` is forbidden, `unreachable_pub`
  warns, Clippy pedantic and `unwrap_used` warn (`.cargo/config.toml` denies warnings),
  `mod_module_files` enforces self-named modules, and rustdoc denies broken
  and private links.
- `clippy.toml` allows `unwrap` in tests only.
- `deny.toml` allows Apache-2.0, MIT and Unicode-3.0.
  A crate-scoped Zlib exception covers `foldhash`, used by the YAML parser's
  hash tables. A crate-scoped MIT-0 exception covers `borrow-or-share`, used
  by bundled schema reference resolution; its license permits reuse and
  distribution without attribution.
- `.config/nextest.toml` disables fail-fast and terminates hung tests.

## Tests

Unit tests sit in each module, or in its `tests.rs` submodule. The integration
binary, `tests/integration/main.rs`, runs `stdio::serve` on a thread with
in-memory pipes and a scriptable handler. The `worker-process` fixtures also
use loopback, private local IPC and owned frontend/worker subprocesses through
the test-only `acme-worker-fixture` executable; use host access in a sandbox.
Wait on recorded events with the fixture's bounded wait rather than sleeping,
and keep deadlines short only to make expiry happen.

Tasks integration fixtures use the Acme example's consumer-owned store in
private temporary folders. Those persistence fixtures require Unix directory
flushes; they test reopen/recovery alongside the in-memory protocol transport.

See the [documentation index](../README.md) for usage guides. When changing
an API, keep its rustdoc and the corresponding guide consistent.

## Measuring test performance

Use `cargo nextest run --locked --all-features` to collect test durations.
Rank individual tests and sum their durations by module. Rerun the slowest
tests in isolation to see which costs persist, and sample the stacks of
CPU-bound tests to locate the work. When comparing Cargo profiles, measure
warm suite time, the rebuild after the same source edit, and a build from an
empty target directory for each profile.

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
