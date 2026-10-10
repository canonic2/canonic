# Input, focus and navigation

Status: target contract. Slice 1 implements shared focus orders/modal return
scopes and editor keys, pending human acceptance. A shared binding dispatcher,
pointer interaction and configurable shortcuts remain proposed.
See [System design](system-design.md).

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
confirmation while an action is pending. Traditional terminals cannot distinguish
every enhanced key combination: Alt-Enter and a documented control-key fallback
must be available for newline without requiring Shift-Enter support.

Hosts supply action IDs, scope and bindings. Components accept semantic editing
or navigation actions as well as a documented default event adapter. Configurable
bindings have validated conflicts/disabled values and help generated from the
same effective map. Loading files and persistence stay with hosts.

Optional leader sequences use an explicit clock, visible pending hint and
configurable timeout. Timeout, focus change and dismissal reset pending sequence.
An unmatched continuation is consumed with feedback while a sequence is pending;
it must not leak into a draft or execute another action. Paste never participates
in command sequences. Leader timing is a later increment, not a prerequisite
for simple arrow/Enter/Esc controls.

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
