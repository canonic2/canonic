# Development

For application integration tests, start with
[Test your integration](../guides/testing.md). This page covers contributing to
the library itself. Run the commands below from `packages/registry`.

This is one unpublished Cargo package with a library and no binary. It uses
Rust 1.99.0, Rust 2024, a committed `Cargo.lock`, rustfmt, strict Clippy,
nextest, doctests, rustdoc, cargo-deny, insta and proptest.

| Task | Command (from this directory) |
| --- | --- |
| All gates | `make check` |
| Formatting | `cargo fmt --check` |
| Lint | `cargo clippy --locked --all-targets --all-features -- -D warnings` |
| Tests | `cargo nextest run --locked --all-features` |
| Doctests | `cargo test --doc --locked --all-features` |
| API docs | `cargo doc --locked --all-features --no-deps --document-private-items` |
| Dependencies | `cargo deny check` |
| Coverage report | `make coverage` (writes `target/llvm-cov/`) |
| Release build | `make build` |

`CARGO_FLAGS=--offline` uses cached dependencies. Use `~/.cargo/bin/cargo`
when Cargo is not on `PATH`. Install the tools with:

```sh
cargo install --locked cargo-nextest@0.9.146 cargo-deny@0.20.2 cargo-llvm-cov@0.9.1 cargo-insta@1.49.0
```

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

See the [documentation index](../README.md) for public usage guides and the
[specifications](../../specs/README.md) for internal contracts. When changing an
API, keep its rustdoc and the corresponding usage guide consistent.

[Documentation index](../README.md)
