# Usage

Open the gallery with `make -C packages/rust-tui run`. It starts at the component
selector. Arrow keys select a component; Enter opens it; Esc returns to the
selector and Esc again exits. Ctrl-C exits from every view. Ctrl-T changes RGB
dark/light tokens; F4 cycles ANSI 16, ANSI 256, RGB and no-color profiles.
Body text uses the terminal's foreground/background in every profile.

The rich picker returns stable choice IDs. The default three-row fixture has no
filter: arrows/Home/End/Page keys navigate and 1–3 immediately choose a row.
Enter confirms the highlighted row; disabled rows cannot be highlighted or chosen.
Danger rows require explicit navigation and are never implicit defaults. F3
returns an inspection intent without choosing. Confirmation updates a fixture
result label and does not change files. Esc cancels; with a nonempty filter it
clears that filter first. Paste never confirms.

F2 cycles default → details → disabled → danger → filter → no-match → scrolled
→ multi → groups → loading → narrow → done → empty → error. It resets the picker
while preserving the independent host draft. F5 simulates a source refresh by
removing the highlighted ID. F6 changes stage/dock/full-screen placement within
the gallery's existing terminal session. Tab/Shift-Tab switches picker/draft
focus; Ctrl-J inserts a draft newline. Return to catalog/reopen resets both.

Filtering defaults on above seven choices; hosts can force it on/off. Type or
paste into the filter; digits are query text when filtering is enabled. Ctrl-G
selects the query; Ctrl-Z/Ctrl-Y undo/redo its editor transactions. Matching is a
case-insensitive subsequence of the label or detail, ignoring query whitespace;
matched graphemes are bold/underlined. A surviving enabled highlighted ID remains
highlighted; otherwise the first safe ranked result is used. Multiple mode uses
Space to toggle a mark and Alt-A to toggle all matching enabled rows. Hidden
marks survive filtering; Enter returns the marked IDs in supplied order, including
hidden marks. An empty marked set may be confirmed when the source is nonempty.
Loading/error/empty sources cannot confirm. The compatibility `Picker`/`Action`
API retains its original-index semantics and remains the catalog's implementation.

The chat fixture has a conversation, status, composer and keyboard hints. Type or
paste; Enter submits to the simulator. Alt-Enter inserts a newline where the
terminal supports that combination; Ctrl-J is the traditional-key fallback.
Pasted newlines also work. Backspace/Delete remove whole graphemes; arrows edit
the draft. Alt-PageUp/Alt-PageDown scroll the transcript and Alt-End follows it.
Plain PageUp/PageDown and End belong to the editor. Ctrl-X
interrupts simulation. F2 cycles fixture states and resets the chat. No agent runs.

For headless inventory use `cargo run --locked --features gallery -- --list`
from the package. The interactive gallery refuses piped input/output before
changing terminal modes.

## Multiline editing and focus

Open **Multiline editor** in the catalog, or run
`cargo run --locked --features gallery -- --component editor` from the package.
Top and bottom editors have independent drafts; Tab/Shift-Tab switches focus.
F3 opens a temporary editor modal. Only it receives typing/paste;
Esc clears selection first, then closes it and restores the prior editor.
Esc clears selection before returning to catalog too. Ctrl-C exits the gallery.

| Input | Behavior |
| --- | --- |
| Left/Right, Backspace/Delete | Move/remove a grapheme |
| Ctrl-Left/Right or Alt-Left/Right | Move to Unicode word boundaries |
| Ctrl-Backspace/Delete or Alt-Backspace/Delete | Delete to a word boundary |
| Up/Down | Move by visual wrapped line, retaining preferred column |
| Home/End, Ctrl-A/Ctrl-E | Visual wrapped row start/end |
| Ctrl-Home/End | Draft start/end |
| PageUp/PageDown | Move one editor viewport |
| Shift plus movement | Extend selection; typing/paste replaces it |
| Ctrl-Shift-A or Ctrl-G | Select all; Ctrl-G works with traditional terminal encodings |
| Ctrl-W, Alt-Backspace | Delete previous word |
| Ctrl-U/Ctrl-K | Delete to logical line start/end |
| Ctrl-Z or Ctrl-minus | Undo one transaction; paste is one transaction |
| Ctrl-Y or Ctrl-period | Redo; a new edit clears redo |
| Enter | Newline in standalone editor; submit in chat |
| Alt-Enter, Shift-Enter, Ctrl-Enter or Ctrl-J | Newline where terminal encoding supports it |

Host owns submission/focus; `Editor::update` leaves plain Enter
unhandled. Gallery chooses newline in its standalone fixture. Neither editor
nor focus state acquires modes. Clipboard I/O/persistent history remain host work.

F2 cycles focused, blurred, empty, wrap, selection, Unicode, scrolled, pasted,
undo, near-limit, limit and paste-cut. The limit fixtures use 120 bytes; ordinary
drafts use 4 KiB. The editor uses its full width with title/footer rules and no
side borders. Prose wraps by word; longer words break at grapheme boundaries.
Up/Down follows those rendered rows; logical line/column metadata counts graphemes.

Oversized input inserts the complete grapheme prefix that fits, in one undo
transaction, and reports discarded bytes. If no grapheme fits, draft/selection
remain intact. The byte counter appears at 90%; a reached limit persists until
an edit makes room. `set_text` truncates host replacement at a grapheme boundary
and resets selection/undo. Undo keeps 64 transactions. Typing/deletion groups
until a one-second pause, movement, selection replacement or paste; trailing
whitespace ends a typing group. Paste is always one separate transaction.

Paste normalizes CRLF/CR to LF and retains tabs at four-cell display stops.
Other stored controls render as dim control pictures or ASCII hex labels; they
never emit escape sequences. The host receives the original controls on submission
and owns validation/execution. Resize preserves text/caret; release events do not
edit. Focus changes preserve the draft, selection and viewport. The terminal's
cursor retains its own shape/blink; the library does not install a custom cursor.

`EditorView` supplies title, placeholder, footer and `Presentation`; `chrome=false`
renders only its text area, as used by the picker's single-row filter. Edit notices
temporarily override the footer, and pasted text is tinted until the next key or
three seconds. Hosts call `Editor::tick(elapsed)` to advance time; the editor owns
no timer. Empty footer props use line/column and selection metadata.

## Presentation profiles

The gallery resolves environment hints once before entering the terminal:
nonempty `NO_COLOR`, then `COLORTERM=truecolor`/`24bit`, then `TERM` containing
`256color`, otherwise ANSI 16. Explicit command options override that choice;
an explicit color profile emits colors even when `NO_COLOR` is set. Auto mode
still respects `NO_COLOR`. RGB output requires a terminal that supports it;
use `--colors ansi256` for the indexed palette when RGB is unavailable.
An explicit `truecolor` option requests RGB output without verifying support.

```sh
make -C packages/rust-tui run CARGO_FLAGS="-- --component editor --colors truecolor"
make -C packages/rust-tui run CARGO_FLAGS="-- --component editor --colors truecolor --light"
make -C packages/rust-tui run CARGO_FLAGS="-- --component editor --colors none --ascii"
```

Tokens cover body/dim/accent/success/warning/error/info, surface and selection;
spacing is measured in cells. ANSI 16 uses the user's palette and reverse
selection. Extended modes use selection tint; no-color uses bold/reverse without
color overrides. ASCII affects decorations and control pictures, preserving
Unicode document text. Components receive explicit profiles and never inspect
environment variables or query the terminal. Automatic background detection
through OSC 11 is not implemented; select light tokens explicitly.

## Local consumers

Drone, Shield and Gate declare `canonic-rust-tui` as an optional, versioned local
path dependency. Enable their `tui` feature; no package registry is involved for
this library. Upstream dependencies are resolved through their Cargo lockfiles.

```rust
use canonic_rust_tui::{picker::{Action, Picker}, event::{Event, KeyCode, KeyEvent, KeyModifiers}};

let mut picker = Picker::new("Review", vec!["Allow once".into(), "Cancel".into()], Some(1));
let result = picker.update(&Event::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)));
assert_eq!(result, Action::Selected(1));
```

The host dispatches events and renders components in its Ratatui frame. Components
return actions; the host decides how to execute or reject them. `Chat::push`,
`append`, `set_status` and `set_input` update display state. A rejected submission
can be restored with `set_input`. `TerminalSession` is an optional host helper
for one full-screen event loop; inline integrations can own their own runtime.
Never acquire another terminal session for a nested picker.
