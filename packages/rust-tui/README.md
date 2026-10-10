# Rust TUI

An unpublished Rust component library and local full-screen gallery for Canonic.
Drone, Shield and Gate use it through local Cargo path dependencies. Ratatui
renders the views; Crossterm supplies terminal events and modes. Importing the
library does not open a terminal or execute product operations.

From the repository root:

```sh
make -C packages/rust-tui run
```

The first screen selects **Option picker**, **Full-screen chat**, **Multiline editor**
or **Dialogs**. Use arrows
and Enter to open a component, Esc to return (or quit at the selector), Ctrl-C
to quit and Ctrl-T to toggle dark/light themes. F2 cycles picker design states or
cycles chat fixtures: ready, empty, streaming, error, disconnected and long text.

The compact picker returns stable choice IDs, with details, groups, disabled and
danger rows, fuzzy filtering, paging and explicit single/multiple confirmation.
F6 changes its gallery placement; Tab switches to an independent host draft.
The chat uses a multiline editor with cursor movement, selection,
paste, grouped undo/redo, Alt-Enter or Ctrl-J newline and Enter submission.
Alt-PageUp/PageDown scroll the transcript; Alt-End follows
output; Ctrl-X interrupts simulated activity. The gallery uses local fixture
data, with no AI account, policy enforcement, Git operation or saved approval.

The editor uses full-width text between title/footer rules, with semantic color
and cell tokens. F2 cycles its twelve design states; F4 cycles color profiles.
Use `--colors none --ascii` for attribute-only ASCII chrome or
`--colors truecolor --light` for light RGB roles. Body colors follow the terminal.

Dialogs reuse those rules and tokens around caller-owned editors, pickers and
inspection bodies. Only the top layer receives input. Close restores surviving
focus; replacement/clear invalidate result tickets. F2 cycles dialog fixtures,
F3 nests a prompt and F6 replaces the current layer. No host operation executes.

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
