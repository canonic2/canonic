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

`runtime/inline.rs` owns cursor anchoring, fixed live rows, normal-screen output,
alternate borrowing and released foreground tokens under the shared terminal
lease. `inline_composition.rs` is a pure bounded print ledger; hosts acknowledge
exact batches only after output acceptance. `gallery/inline.rs` composes existing
views and deterministic host data. Its native loop is opt-in through `--inline`;
plain fallback is a fixture host adapter, not an implicitly enabled consumer CLI.
The native host uses the components' height APIs and `Chat::inline_height`, then
updates `InlineSession::set_height` as content or width changes. Pure measurement
and terminal fitting remain separate owners. The shared `tests/fixtures/inline_pty.py`
harness replies to cursor reports, tracks normal/alternate buffers and checks
cooked input plus original termios
after restoration. It does not establish real emulator reflow or Windows support.

`tool_call_card.rs` owns call/group state and opaque effects; its model/view
submodules hold safe snapshots and bounded semantic row projection. Chat's tools
adapter owns message/part routing and stable group-member anchors. The dedicated
gallery host simulates retry, clipboard and searchable output inspection. Its
77 states and tests exercise lifecycle, concurrent folding, output coalescing,
stale tickets, draft preservation, responsive profiles and Unix PTY restoration.
`run_activity.rs` owns whole-run snapshots, cancellation deduplication and opaque
host effects. Chat's run adapter preserves the draft and routes independent
connection status. The gallery holds cancellation until F7 acknowledges it,
F5 completes first or F6 reports a cancellation failure.

`composer.rs` and its modules own submission, queue, history/stash, payloads,
completion, autosave snapshots and editor conflicts. `editor/atoms.rs` carries
lossless chips through transactions; `composer/codec.rs` encodes caller-owned
streams. The foreground editor adapter is invoked explicitly by a host and
wrapped in `TerminalSession::handoff`. `gallery/composer.rs` simulates those host
ports with 99 named scenarios. See the [implementation contract](../specs/components/composer.md).

`theme/tokens.rs` defines semantic color/symbol/cell contracts; `theme.rs`
resolves profiles and reusable styles. `ui.rs` owns the labeled rule primitive.
Views consume those tokens/primitives; palette literals stay out of components.

`editor.rs` owns draft/selection/viewport state; `editor/history.rs` owns grouped
transactions and `editor/input.rs` maps editor keys. `editor/layout.rs` derives
word-wrapped source/cell rows and printable controls. `editor/view.rs` owns
presentation and explicit host props. `Editor::tick` advances injected time;
component code owns no timers, environment reads or terminal queries.
`focus.rs` owns instance-local focus orders and modal return scopes; hosts dispatch
input only to the active target. `gallery/editor.rs` composes those public APIs
into twelve design states and a two-editor/modal fixture.
Editor tests include rendered cursor/style checks, a reviewed snapshot, Unicode
properties and history/limit behavior. The PTY test also exercises bracketed
multiline paste, focus switching and modal input before resize/restoration.

`picker/model.rs` owns validated choices, safe display fields and deterministic
matching. `picker/choice.rs` owns selection/marks/query/status; its `input.rs`
returns stable-ID intents and its `view.rs` owns compact geometry, groups and
viewport hints. Filtering reuses `Editor` with `EditorView.chrome=false`.
`gallery/option_picker.rs` owns fourteen fixtures, placement, focus and the
independent host draft. Components perform no discovery, retries or execution.
The original `Picker`/`Action` API stays intact for index-based callers/catalog.
Rich picker tests cover identity refresh, disabled/danger defaults, query undo,
hidden marks, physical-row paging, Unicode, narrow styles and a reviewed snapshot.

`dialog.rs` owns a generic bounded modal stack, scoped focus, opening tickets,
validation and dismissal records. It wraps `Focus` rather than duplicating return
scope logic. `dialog/view.rs` owns measured centered panels, background
fading, surface/box fallbacks and shared-rule chrome.
Bodies receive caller frames/events and own their state/scrolling. Stack dispatch
never sends unhandled modal events to background views; hosts cancel work from
returned dismissal records. `dialog/timing.rs` owns injected arrival/guard/notices
and attention intents; `dialog/confirmation.rs` owns local caution qualification.
`gallery/dialogs.rs` composes 36 named scenarios and
three independent base drafts, including a fixed dock composer;
`gallery/dialog_body.rs` supplies editors, choice
pickers and a fixture-only read-only text body. A general content renderer is a
later slice. Measurement/render callbacks exist only during rendering; no retained
callback, timer, terminal acquisition or process belongs to a layer.
Dialog tests cover nested routing/focus repair, atomic validation, stale/foreign
tickets, outside pointer isolation, measured growth, Unicode caret cells, tiny
geometry, color fallbacks and reviewed snapshots. A
separate macOS PTY scenario tests nested paste, parent preservation and restoration.

`commands.rs` owns sanitized descriptors, recent/grouped ranked search and palette/
keys projections. `commands/view.rs` renders command columns using shared tokens
and rules; `commands/bindings.rs` validates dispatch and explicit pending timing.
`commands/remaps.rs` resolves ordered overrides with retained conflict/source
evidence. Hosts supply scope maps, load bindings and execute returned intents.
`gallery/commands.rs` owns docked palette/full-screen keys composition, timed
Dialog input policy and delayed
hint strips; its fixtures supply deterministic commands and remaps. Tests cover
reserved/conflicting maps, source inspection, scoped search, key spelling, 400 ms
hints, 3-second timeout, draft isolation and reviewed palette/keys snapshots.

State and rendering tests run without a terminal. TestBackend supplies terminal
dimensions; insta captures the selector, with behavioral assertions alongside
snapshots. Review generated snapshots with `cargo insta review`; do not accept
unexpected changes automatically. Property tests cover selection bounds.

Presentation tests cover terminal-default colors, ANSI/indexed/RGB/no-color
selection, ASCII, host labels/placeholder/footer, copy-safe rows and notice
expiry. [Presentation architecture](../specs/presentation.md) documents these
boundaries and the current design-system/editor scope.

The Unix PTY integration test needs Python 3. It verifies selector navigation,
picker confirmation, chat submission, resize, Ctrl-C and terminal mode restoration.
Request host access in an agent sandbox before PTY/process-signal tests or tools
writing protected caches/agent outputs. Run process-heavy suites separately.
The terminal guard also restores on Rust unwinding; forced termination and
Windows terminal behavior need separate acceptance. Components remain useful in
pure tests even when no real terminal is available.

Crossterm uses its `use-dev-tty` polling backend. The macOS PTY regression showed
pending input becoming unread after resize with the default backend; the polling
backend passes resize, modal input, Ctrl-C and post-exit canonical line handling.
This is native macOS evidence; Windows and Linux acceptance remain separate.

The library emits no logs or terminal input on import. Gallery fixture errors are
display-only; product logging stays with the host. Review Gate changes in both
owning repositories and run each one's `git diff --check` through its entry folder.
