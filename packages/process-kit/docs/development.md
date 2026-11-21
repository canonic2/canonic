# Development

Rust nightly-2026-11-09 (rustc 1.102) and edition 2024 are pinned. Cargo.lock pins dependencies. Run
these commands from `packages/process-kit/`, where rustup selects the pinned
toolchain.

| Command | Purpose |
| --- | --- |
| `cargo build --locked --all-features` | Incremental development build |
| `cargo nextest run --locked --all-features --test integration 'lifecycle::'` | Focused acceptance |
| `cargo nextest run --locked --all-features` | All non-documentation tests |
| `cargo fmt` / `cargo fmt --check` | Apply / check rustfmt |
| `cargo clippy --locked --all-targets --all-features` | Clippy, with warnings denied by `.cargo/config.toml`; type-checks all targets |
| `cargo test --doc --locked --all-features` | Compile documentation examples |
| `cargo doc --locked --all-features --no-deps --document-private-items` | Generate local rustdoc HTML |
| `cargo deny check` | cargo-deny advisory, license and source checks |
| `cargo llvm-cov nextest --locked --all-features --html` | LLVM coverage through cargo-llvm-cov and nextest |
| `cargo sweep --time 3` | Delete build output unused for more than three days (from cargo-sweep) |

Run `cargo sweep` on its own, to free space, while no build or test run uses
`target/`.

These commands are independent. Tests use one integration binary, temporary
Acme roots, real sockets and locks, and the test binary itself for process
fixtures. Socket/process/signal tests require host access when sandboxed.
Preserve Cargo caches. Never use default user roots, real login registrations
or another user's resources in tests.
