# Development

Rust 1.99.0 and edition 2024 are pinned. Cargo.lock pins dependencies. Run
individual Make targets from the package or use `make -C packages/manager`.

| Target | Purpose |
| --- | --- |
| `dev` | Incremental development build |
| `test-focus FILTER='lifecycle::' TEST_TARGET='--test integration'` | Focused acceptance |
| `test` | All non-documentation tests through cargo-nextest |
| `format` / `format-check` | Apply / check rustfmt |
| `lint` | Clippy with warnings denied |
| `types` | Check all targets |
| `doctest` | Compile documentation examples |
| `docs` | Generate local rustdoc HTML |
| `deps` | cargo-deny advisory, license and source checks |
| `coverage` | LLVM coverage through cargo-llvm-cov and nextest |

These targets are independent. Tests use one integration binary, temporary
Acme roots, real sockets and locks, and the test binary itself for process
fixtures. Socket/process/signal tests require host access when sandboxed.
Preserve Cargo caches. Never use default user roots, real login registrations
or another user's resources in tests.
