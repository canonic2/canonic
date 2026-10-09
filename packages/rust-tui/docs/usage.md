# Usage

Open the gallery with `make -C packages/rust-tui run`. It starts at the component
selector. Arrow keys select a component; Enter opens it; Esc returns to the
selector and Esc again exits. Ctrl-C exits from every view. Ctrl-T changes the
theme. Resize the terminal to inspect clipping and layout.

Type to search in the picker; Enter returns an original choice index. Esc cancels.
The example defaults to continuation without the server. Confirmation updates a
fixture result label and does not change files. F2 opens an empty picker fixture;
return to the selector and reopen to reset it.

The chat fixture has a conversation, status, composer and keyboard hints. Type or
paste; Enter submits to the simulator. Alt-Enter inserts a newline where the
terminal supports that combination; pasted newlines also work. Backspace removes
a grapheme. PageUp/PageDown scroll and End follows the newest message. Ctrl-X
interrupts simulation. F2 cycles fixture states and resets the chat. No agent runs.

For headless inventory use `cargo run --locked --features gallery -- --list`
from the package. The interactive gallery refuses piped input/output before
changing terminal modes.

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
