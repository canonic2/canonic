# Canonic TUI

The gallery includes a searchable command palette and keyboard help backed by
host-supplied descriptors, plus validated scoped shortcuts and leader sequences.
See [usage](docs/usage.md#commands-and-keyboard-help).

An unpublished Rust component library and local full-screen gallery for Canonic.
The Cargo package is `canonic-tui`; Rust code imports it as `canonic_tui`.
Drone and Shield use it through local Cargo path dependencies. Ratatui
renders the views; Crossterm supplies terminal events and modes. Importing the
library does not open a terminal or execute product operations.

From the repository root:

```sh
make -C packages/tui run
```

The first screen selects **Option picker**, **Full-screen chat**, **Multiline editor**,
**Dialogs**, **Commands and bindings**, **Composer**, or **Completion Menu**. Use arrows
and Enter to open its scenario picker, then choose a scenario with Enter.
`main` is first. Esc returns through pickers and quits at the component picker.
Composer uses Esc for local actions and F10 for scenarios; Ctrl-C twice on its
empty idle box quits. Ctrl-C quits other components. Ctrl-T toggles themes.
F10 opens the active component's scenarios. F2 cycles working fixtures.
Returning to a picker retains its search, selection and scroll position;
scenario picker state is remembered separately for each component.
Named scenarios also open with `--component <name> --scenario <name>`;
`--list` lists them without a terminal. Pending designs display their requirements
and source board. See the [design coverage map](specs/features/gallery-designs.md).

The compact picker returns stable choice IDs, with details, groups, disabled and
danger rows, fuzzy filtering, paging and explicit single/multiple confirmation.
F6 changes its gallery placement; Tab switches to an independent host draft.
The chat uses a multiline editor with cursor movement, selection,
paste, grouped undo/redo, Shift-Enter, Alt-Enter or Ctrl-J newline and Enter submission.
Alt-PageUp/PageDown scroll the transcript; Alt-End follows output. Ctrl-F searches
loaded content; Ctrl-X V selects blocks for Markdown/code copy and opening;
Ctrl-X T toggles timestamps. Chat supports identified rich parts, stable anchors,
unread and host-owned paging; Esc stops active composer work. The gallery uses local fixture
data, with no AI account, policy enforcement, Git operation or saved approval.

Composer includes immediate submission/acknowledgement, one-per-turn queues,
editor-first Up recall, searchable history, stash swaps/drop undo, autosave
snapshots, paste/image chips, shell mode, slash/file completion and external-editor
conflicts. The gallery has 59 named composer scenarios with simulated host work.
F5 accepts or ends a turn, F6 refuses/fails, F11 returns editor text and F12
restores saved state with the queue paused. See [usage](docs/usage.md#composer-workflows)
and the [public implementation contract](specs/components/composer.md).

Completion Menu has 15 interactive states for commands/files, streaming, failure,
retry, disabled candidates and insertion. Tab and Enter insert; a later Enter
invokes a known command or submits the message. Its pure reusable API owns ranking,
source tickets and fixed-height rendering; hosts own discovery and I/O. See
[usage](docs/usage.md#completion-menu) and its [contract](specs/components/completion-menu.md).

The editor uses full-width text between title/footer rules, with semantic color
and cell tokens. F2 cycles its twelve design states; F4 cycles color profiles.
Use `--colors none --ascii` for attribute-only ASCII chrome or
`--colors truecolor --light` for light RGB roles. Body colors follow the terminal.

Dialogs reuse those rules and tokens around caller-owned editors, pickers and
inspection bodies. Only the top layer receives input. Close restores surviving
focus; replacement/clear invalidate result tickets. F2 cycles dialog fixtures,
F3 nests a prompt and F6 replaces the current layer. Dialogs center over faded
content and the fixed dock. Indexed/RGB surfaces contain inset
fields; ANSI 16/no-color use boxes. No host operation executes.

```sh
cd packages/tui
cargo run --locked --features gallery -- --list
cargo run --locked --features gallery -- --component chat
cargo run --locked --features gallery -- --component composer --scenario sending
cargo run --locked --features gallery -- --component completion-menu --scenario main
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
