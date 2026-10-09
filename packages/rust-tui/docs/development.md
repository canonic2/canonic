# Development

This is one unpublished Cargo package, with optional `gallery` feature and
`tui-gallery` binary. Rust 1.99.0, Rust 2024, Cargo.lock, rustfmt, strict Clippy,
nextest, doctests, rustdoc, cargo-deny, insta and proptest match Gate/Shield.

`make check` runs the package gates; `make build` builds a release gallery.
`make coverage` uses `cargo llvm-cov nextest --html` and writes to
`target/llvm-cov/`. There is no coverage percentage gate. `CARGO_FLAGS=--offline`
uses cached dependencies. `make run` opens the component selector. No root
workspace, distribution script or registry publication exists.

Missing development tools can be installed with the same versions as Gate:

```sh
cargo install --locked cargo-nextest@0.9.146 cargo-deny@0.20.2 cargo-llvm-cov@0.9.1 cargo-insta@1.49.0
```

Modules under src/ own reusable components, rendering, text normalization and
terminal lifetime. gallery.rs owns fixture composition and the application loop;
main.rs stays thin. All components accept caller-owned events and frames. Add a
component to the gallery's selector and fixtures when it is implemented.

State and rendering tests run without a terminal. TestBackend supplies terminal
dimensions; insta captures the selector, with behavioral assertions alongside
snapshots. Review generated snapshots with `cargo insta review`; do not accept
unexpected changes automatically. Property tests cover selection bounds.

The Unix PTY integration test needs Python 3. It verifies selector navigation,
picker confirmation, chat submission, resize, Ctrl-C and terminal mode restoration.
Request host access in an agent sandbox before PTY/process-signal tests or tools
writing protected caches/agent outputs. Run process-heavy suites separately.
The terminal guard also restores on Rust unwinding; forced termination and
Windows terminal behavior need separate acceptance. Components remain useful in
pure tests even when no real terminal is available.

The library emits no logs or terminal input on import. Gallery fixture errors are
display-only; product logging stays with the host. Review Gate changes in both
owning repositories and run each one's `git diff --check` through its entry folder.
