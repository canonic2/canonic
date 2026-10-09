# Rust TUI

An unpublished Rust component library and local full-screen gallery for Canonic.
Drone, Shield and Gate use it through local Cargo path dependencies. Ratatui
renders the views; Crossterm supplies terminal events and modes. Importing the
library does not open a terminal or execute product operations.

From the repository root:

```sh
make -C packages/rust-tui run
```

The first screen selects **Option picker** or **Full-screen chat**. Use arrows
and Enter to open a component, Esc to return (or quit at the selector), Ctrl-C
to quit and Ctrl-T to toggle dark/light themes. F2 shows the empty picker or
cycles chat fixtures: ready, empty, streaming, error, disconnected and long text.

The picker supports typing/search, paste, arrow navigation and explicit Enter
confirmation. The chat has a basic composer with paste, grapheme backspace,
Alt-Enter newline and Enter submission. PageUp/PageDown scroll; End follows
output; Ctrl-X interrupts simulated activity. The gallery uses local fixture
data, with no AI account, policy enforcement, Git operation or saved approval.

```sh
cd packages/rust-tui
cargo run --locked --features gallery -- --list
cargo run --locked --features gallery -- --component chat
make check
make coverage
```

Each consumer has an optional `tui` Cargo feature enabling the dependency:

```sh
cargo check --locked --features tui --manifest-path private/drone/Cargo.toml
cargo check --locked --features tui --manifest-path private/shield/Cargo.toml
cargo check --locked --features tui --manifest-path packages/gate/Cargo.toml
```

Existing product interfaces retain their behavior; their UI migrations are
separate increments. See [usage](docs/usage.md), [development](docs/development.md)
and [internal specs](specs/README.md). No crates.io publication or gallery
installation is required.
