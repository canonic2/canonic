# Canonic TUI

The gallery includes a searchable command palette and keyboard help backed by
host-supplied descriptors, plus validated scoped shortcuts and leader sequences.
See [usage](docs/usage.md#commands-and-keyboard-help).

An unpublished Rust component library and local full-screen gallery for Canonic.
Inline Composition adds an explicit normal-screen host, ordered settled output,
borrowed full-screen views and plain numbered fallback. Live regions use component
measurements to grow and shrink under the same lease. Its 30 scenarios are also
available in the regular gallery. Run the native host with
`cargo run --locked --features gallery -- --inline --component inline-composition --scenario streaming`.
Both native Unix hosts release modes on Ctrl-Alt-Z or external SIGTSTP; shell `fg`
restores the draft and active view. Ctrl-Z retains editor undo.
See [usage](docs/usage.md#inline-composition) and the
[terminal contract](specs/features/terminal.md#inline-runtime-api).
Code/Diff Inspection supplies code and unified-patch views, logical navigation,
folds, whole-file switching, loaded find, line selection and acknowledged
copy/editor requests. Its 51 live scenarios include Chat/tree return with
preserved caller state. See [usage](docs/usage.md#code-and-diff-inspection) and
the [contract](specs/components/code-diff-inspection.md).
Status Panels & Notices provides supplied sections, responsive Chat/Dialog placement,
explicit notice expiry, persistent errors and acknowledged actions in 27 live
scenarios. Ctrl-X I opens status and Ctrl-X N lists notices when the host enables
the adapter. See [usage](docs/usage.md#status-panels-and-notices) and the
[contract](specs/components/status-panels-and-notices.md).
The Cargo package is `canonic-tui`; Rust code imports it as `canonic_tui`.
Drone and Gate declare it through optional local Cargo path dependencies. Ratatui
renders the views; Crossterm supplies terminal events and modes. Importing the
library does not open a terminal or execute product operations.

Session Browser provides a dedicated screen over host-supplied conversations. Its
72 live scenarios cover stable-ID navigation, local/remote search, paging,
rename/fork/move/pin/delete, acknowledged results and returning to retained Chat
instances. Ctrl-X O opens it when the host enables the adapter; the `sessions`
palette command uses the same entry point. All discovery and storage stay with
the host. See [Session Browser](specs/components/session-browser.md) and
[usage](docs/usage.md#session-browser).

Tree Browser navigates host-supplied files, diff resources and tasks with stable
IDs, saved folds, ancestor filtering, lazy children, paging and acknowledged
actions. Its 66 live scenarios compose full-screen and responsive panel placement
with retained Chat. Ctrl-X F is available when the host enables the adapter.
See [Tree Browser](specs/components/tree-browser.md) and
[usage](docs/usage.md#tree-browser). Loading, inspection and execution stay with
the host.

Permission Review offers 56 deterministic scenarios. Numbers highlight; Enter
confirms. Broader choices quote exact host scope in a second check on Back;
destructive choices require two deliberate Enter presses within three seconds.
F5 acknowledges, F6 fails, F7 refuses, F8 expires, F9 replaces and F12 supplies
a stale result. R explicitly retries failed application. It shares Question
Form's above-dock Chat queue and preserves the draft and logical viewport.
See [Permission Review](specs/components/permission-review.md) and
[usage](docs/usage.md#permission-review). No fixture executes or stores a grant.

From the repository root:

```sh
make -C packages/tui run
```

The first screen selects **Option picker**, **Full-screen chat**, **Multiline editor**,
**Dialogs**, **Commands and bindings**, **Composer**, **Completion Menu**, **Tool Call Card**, **Question Form**, **Permission Review**, **Session Browser**, **Tree Browser**, **Code & diff inspection**, **Status Panels & Notices**, or **Inline Composition**. Use arrows
and Enter to open its scenario picker, then choose a scenario with Enter.
`main` is first. Esc returns through pickers and quits at the component picker.
Composer uses Esc for local actions and F10 for scenarios; Ctrl-C twice on its
empty idle box quits. Open dialogs arm Ctrl-C before closing one layer; busy Chat
uses Ctrl-C to request cancellation. Ctrl-C quits other idle components. Ctrl-T toggles themes.
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
conflicts. The gallery has 99 named composer scenarios with simulated host work.
F5 accepts or ends a turn, F6 refuses/fails, F11 returns editor text and F12
restores saved state with the queue paused. See [usage](docs/usage.md#composer-workflows)
and the [public implementation contract](specs/components/composer.md).

Completion Menu has 30 interactive states for commands/files, streaming, failure,
retry, disabled candidates and insertion. Tab and Enter insert; a later Enter
invokes a known command or submits the message. Its pure reusable API owns ranking,
source tickets and fixed-height rendering; hosts own discovery and I/O. See
[usage](docs/usage.md#completion-menu) and its [contract](specs/components/completion-menu.md).

Tool Call Card supplies nine lifecycle states, text/code/diff previews, ordered
concurrent groups, coalesced streaming and identified copy/details/retry effects.
Its 79 gallery scenarios run inside Chat with an independent Composer draft,
including twenty run activity and cancellation scenarios.
Ctrl-X V selects cards; Enter expands, `o` opens searchable full-output details,
`r` requests a new retry card, and `y`/`c` copy call/output in the local simulation.
See [usage](docs/usage.md#tool-call-card) and its [contract](specs/components/tool-call-card.md).

Question Form stages single/multiple choices and multiline typed answers above
Chat's preserved Composer. Multiple questions require a final Review; one question
submits on deliberate choice. The shared queue defers arrivals during typing,
retains the active review and validates identified host acknowledgements. Its 52
scenarios simulate failure/refusal, partial answers, expiry, replacement and copy.
F5 accepts, F6 fails, F7 refuses, F8 expires, F9 replaces and F12 probes a stale
reply. See [usage](docs/usage.md#question-form) and the [contract](specs/components/question-form.md).

The editor uses full-width text between title/footer rules, with semantic color
and cell tokens. F2 cycles its twenty-five working states; F4 cycles color profiles.
Ctrl-Y yanks and Alt-Y cycles ten local kills; Ctrl-Shift-Z or Alt-Z redoes.
Use `--colors none --ascii` for attribute-only ASCII chrome or
`--colors truecolor --light` for light RGB roles. Body colors follow the terminal.

Dialogs reuse those rules and tokens around caller-owned editors, pickers and
inspection bodies. Only the top layer receives input. Close restores surviving
focus; replacement/clear invalidate result tickets. F2 cycles dialog fixtures,
F3 nests a prompt and F6 replaces the current layer. Dialogs center over faded
content and the fixed dock. Indexed/RGB surfaces contain inset
fields; ANSI 16/no-color use boxes. No host operation executes.

Timed dialogs defer arrivals while typing, guard Enter/digits for 300 ms and arm
Ctrl-C closure before closing one layer. Irreversible choices require a second
Enter within three seconds; named confirmations require exact input. The 36
named scenarios simulate notifications, queues and host removal without external effects.

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
cargo check --locked --features tui --manifest-path packages/gate/Cargo.toml
```

Existing product interfaces retain their behavior; their UI migrations are
separate increments. See [usage](docs/usage.md), [development](docs/development.md)
and [internal specs](specs/README.md). No crates.io publication or gallery
installation is required.
