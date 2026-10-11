# Input, focus and navigation

Status: target contract. Slice 1 implements shared focus orders/modal return
scopes and editor keys. Slice 2c implements scoped command maps and validated
shortcut sequences; its human gate is pending. Pointer interaction remains proposed.
See [System design](../system-design.md).

Chat implements logical transcript anchors, loaded search and semantic block
selection/copy with instance-scoped host acknowledgements. It composes one
Composer and returns focus/draft/caret after local Escape. Native terminal mouse
selection remains available; Chat does not acquire pointer capture.

Slice 2b implements modal dispatch through `dialog::Stack`, wrapping existing
focus scopes. Only the top body handles input; Tab traps within that scope and
Escape clears a body's local selection/query before closing one layer. Outside
pointer events cannot activate background views. Result tickets identify active
openings; closing/replacing rejects stale updates. Human acceptance is pending.

## Intended behavior

All operations are keyboard reachable. A composition has one focused target;
visible focus is distinct from selection and disabled state. Tab/Shift-Tab move
among enabled targets in host-defined order. Opening an overlay saves the return
target and traps focus in the overlay. Closing restores a surviving target or a
documented fallback, never an invisible/destroyed editor.

Dispatch prioritizes the active scope: selection-copy/dismiss, top modal,
completion, focused editor/view, screen navigation, then host global actions.
The composition can configure emergency exit, but its meaning is visible and
does not also answer a review. Consumed events stop. Background views receive
data updates but no typing/paste while obscured. Pointer clicks outside a modal
cannot activate an underlying operation. Resize does not commit or dismiss.

An Esc dismisses the current transient state first: selection, completion,
dialog or local edit, as applicable. It does not simultaneously cancel a run
and exit the screen. Host maps cancellation and exit independently. Ctrl-C
copy/editor-clear/interrupt/exit conflicts must be resolved by scoped bindings
and visible hints, not by each component adding its own global handler.

## Binding contract

Input normalization preserves key modifiers, press/repeat/release, paste as one
event, pointer coordinates and resize/focus changes. Ignore release for commands;
allow repeats for movement/editing, suppress repeated one-shot submission or
confirmation while an action is pending. The shared
[keyboard contract](#keyboard-contract-and-terminal-quirks) governs Enter and
terminal reporting across every component and host adapter.

## Keyboard contract and terminal quirks

Agreed on 2026-10-10. This is a hard rule across the library, gallery and consumer
adapters, including chat composers and dialog prompts. A host may choose plain
Enter's action; it may not redefine modified Enter as submission.

| Input | Multiline editor/composer/prompt | Picker or other single-line action control |
| --- | --- | --- |
| Shift-Enter | Insert a newline | Never confirm or activate |
| Alt-Enter | Insert a newline | Never confirm or activate |
| Ctrl-J | Insert a newline; traditional-terminal fallback | No implicit confirmation |
| Plain Enter press | Host-defined: submit in chat/prompt, newline in standalone editor | Explicit confirmation of a valid enabled selection |
| Modified Enter repeat/release | Release never edits; repeat never submits | Never confirm or activate |
| Paste | Insert the supplied text, normalizing CRLF/CR to LF | Never confirm or activate |

Multiline inputs use the same editor event adapter. A modifier-aware Enter event
must reach that editor without a host intercepting it as submission. Submission
requires an unmodified Enter press and must happen only once. Empty/disabled or
pending actions retain their own validation. Completion or another overlay must
not turn modified Enter into an affirmative action. Consumed input never reaches
a background draft.

Traditional terminals can send identical bytes for Enter and Shift-Enter, or omit
Alt/Option. Those bytes cannot reveal which physical keys were pressed. The
terminal owner requests enhanced keyboard reporting on supported hosts, preserves
the reported modifiers, and restores the previous reporting state on every exit.
Components neither acquire keyboard modes nor infer missing modifiers from timing.
Ctrl-J remains a documented newline path when modifiers cannot be reported.
Do not require a terminal-settings change as the default product workflow or
claim a physical shortcut is supported solely because synthetic events pass.

Unix `TerminalSession` pushes disambiguated keyboard reporting and pops it during
cleanup. Windows uses the existing event backend without these Unix-only commands.
No native Windows modifier acceptance is established. iTerm2 recommends the
[Kitty keyboard protocol](https://iterm2.com/documentation-csiu.html); terminals and
user profiles can still limit or remap reporting. Native keyboard observations
belong in the [human gates](../human-testing.md), with terminal and profile named.

Automated evidence includes editor newline handling, chat submission guards,
rich-picker modified/repeated Enter rejection, and the dialog PTY scenario sending
encoded Shift/Alt-Enter, then plain Enter. The PTY also verifies keyboard-mode
push/pop and restored shell input. This proves decoding and routing; physical
Shift/Option behavior in the user's iTerm2 profile still awaits human confirmation.

Acceptance requires both modified Enter combinations to grow the active multiline
draft without closing/submitting, plain Enter to perform the documented host
action once, and modal/background drafts to remain isolated. Test a terminal with
enhanced reporting and a traditional-reporting path using Ctrl-J. Repeat/release
and paste must not trigger confirmation, and closing the session must restore
keyboard reporting along with its other owned modes.

## Configurable bindings

Hosts supply action IDs, scope and bindings. Components accept semantic editing
or navigation actions as well as a documented default event adapter. Configurable
bindings have validated conflicts/disabled values and help generated from the
same effective map. Loading files and persistence stay with hosts.

Optional leader sequences use an explicit clock, visible pending hint and
configurable timeout. Timeout, focus change and dismissal reset pending sequence.
An unmatched continuation is consumed with feedback while a sequence is pending;
it must not leak into a draft or execute another action. Paste never participates
in command sequences. Slice 2c exposes a 400 ms hint threshold and uses a 3-second gallery timeout;
the dispatcher accepts a host-supplied timeout.

`commands::bindings::Dispatcher` validates one effective map per focus scope.
It rejects duplicate IDs, duplicate/prefix-conflicting sequences, empty sequences,
sequences longer than four strokes, and Enter/Escape/Ctrl-J/Ctrl-C bindings.
Disabled commands reserve their bindings during validation but cannot invoke.
Press events return one invocation ID; repeats and releases never invoke.
The caller advances `tick(Duration)` and calls `reset()` on scope changes or
dismissal. A pending mismatch or paste cancels and consumes that input; Ctrl-C
clears pending state and reaches the host. Help uses `Binding::label()` from the
same descriptors. `commands::remaps::resolve` accepts ordered host-loaded overrides,
removes losing bindings, and retains both sides' conflict/source evidence.
Reserved overrides remain visible as ignored and preserve the prior binding.
File loading and persistence remain host responsibilities. Help groups by display
scope; the caller supplies the active command map and omits hidden commands.

Ctrl-P uses the docked palette in the gallery by product decision. F1 uses
full-screen keys.
Palette groups are recent-first and disappear during ranked search. Keys use a
separate scope projection of the same effective map; Tab changes the displayed
scope and source inspection returns an intent. Neither projection executes work.

## Scrolling and resize

Viewport state is a logical anchor (stable item/part plus text position), visible
range and follow flag. Line/page/half-page movement uses available body height;
zero-height areas are inert. Home/End meanings are scoped: editor buffer movement
does not steal transcript navigation. Explicit follow jumps to latest output;
manual scrolling pauses follow and shows unread additions. Reaching bottom alone
does not resume follow unless the host chooses and documents that policy.

Reflow after resize, block expansion, theme/font-width policy change or prepend
preserves logical content under inspection. If eviction removes the anchor,
clamp to the nearest retained item and show that older content is unavailable.
Coordinates and row counts use sufficient internal range; never silently cap a
long transcript at a `u16` scroll offset. Terminal rectangles still use Ratatui
dimensions at rendering boundaries.

Mouse capture is opt-in with keyboard equivalents. Wheel affects the hovered
scrollable pane only when it is in the active scope. Hit regions derive from
current rendered layout; stale geometry is invalidated on resize. Dragging text
does not also select/confirm rows. When capture is disabled the terminal retains
native selection; the library cannot promise native selection of alternate-screen
content remains available in every emulator. Scroll acceleration is deferred;
bounded configurable step scrolling is sufficient first.

## Selection and copy

Editor selection and read-only transcript selection are separate instances over
logical text positions. Keyboard extension and optional pointer drag use source
mapping, not stored cell coordinates. Copy returns sanitized semantic content
with original logical newlines, excluding borders, timestamps and role decorations
unless explicitly requested. Code/diff/message/transcript copy modes are labeled.
Collapsed/evicted content is not silently included; full-history export is a host
request. Never copy terminal control sequences or hidden host secrets.

Copy returns a payload intent and pending status. Host clipboard success/failure
produces a notice; failure keeps a selection available for retry. OSC clipboard
delivery is an explicit host capability, not a component escape emission. Opening
a link likewise returns an ID/destination intent; rendering never opens it.

## Acceptance criteria

- **I-01:** Modal/focus event traces prove one consumer per event, draft isolation,
  valid return focus and no background shortcut/confirmation leakage.
- **I-02:** Press/repeat/release, paste and binding conflicts produce documented
  outcomes; pending sequences reset on timeout/focus/close without inserting text.
- **I-03:** Scroll/reflow/prepend/eviction maintains logical anchors or signals
  loss; long content navigates past 65,535 rows without offset saturation.
- **I-04:** Keyboard and optional pointer selection copy logical Unicode text;
  drag release does not confirm, and clipboard failure is visible/retryable.
- **I-05:** All controls have keyboard paths and effective hints; traditional
  newline fallback, capture-off and tiny/zero-area layouts are verified.

## Decisions still needed

Exact default chat bindings and leader timeout are proposals for a gallery
review, not fixed product commands. Select initial mouse defaults and native
selection expectations by terminal profile before claiming compatibility.
