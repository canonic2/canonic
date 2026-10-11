# Dialog

## Purpose

Present caller-owned bodies in an isolated modal stack while preserving parent
state and restoring surviving background focus.

## Current status

Stack mechanics, injected-clock arrival/guard/notice policies and reusable caution
policies are implemented. The gallery has 36 named scenarios. Human visual
acceptance and native inline-screen hosting remain separate checkpoints.

## Behavior and state transitions

### Dialog composition

Implemented `dialog::Stack<B,T>` owns up to sixteen caller-supplied bodies and
wraps instance-local `Focus<T>`. `Dialog` supplies a unique open ID, title, body
target order, preferred cell size and close policy. Duplicate IDs/targets or depth
overflow reject push/replace before changing existing bodies/scopes. Closed body
ownership returns in `Dismissed` with its ticket and reason; clear reports top to
bottom. Replacement reports the old layer and creates a fresh opening.

Tickets are unique per opening, including between independent stacks. Only the
active ticket permits body updates or submission closure; foreign, obscured,
closed and replaced results are rejected. Hosts can retry current data after an
obscured parent resumes. The stack starts no asynchronous work and retains no
completion callback; hosts cancel their own work from dismissal records and
clear before tearing down a composition with outstanding tasks.

Dispatch calls only the top handler. Tab/Shift-Tab advances its scope; releases
are consumed. Escape is offered to the body first so a selection/filter clears
before closing; policy may forbid escape closure. Outside pointer events are
consumed; only an opted-in outside left press dismisses. It never confirms.
The gallery simulates that press without enabling terminal mouse capture.

Every body and host adapter follows the universal
[keyboard contract](../features/interaction.md#keyboard-contract-and-terminal-quirks).
Enter submits the named action, including in tall fields. Ctrl-J inserts a newline.
Reported Shift/Alt-Enter may also insert a newline; hints rely on Ctrl-J.

`DialogView` supplies scope, message and normal/compact action footer.
`Stack::render_with` measures body height at the available width before placing
the centered panel. Panels are at most 72 columns and leave four viewport rows.
Below three viewport rows the body takes the full viewport without a frame,
title, message, footer or padding. The compatibility
`render` method retains the preferred size height without a body measurement.
Only the top body renders, with parent titles in its trail. The view fades the
already-rendered visible background, fills the surface in indexed/RGB modes,
and uses a full box in ANSI 16/no-color. Body components own scrolling and action
reachability; gallery prompt fields have a 2 KB budget and grow to six rows, then
scroll. Gallery bodies are editors, pickers and local inspection text.
General review/content bodies and pointer capture remain their owning later slices.

Dialog host owns an explicit stack, target ID, size policy, body component,
title, close policy and return focus. Only topmost layer handles input. Resize
clamps to available dimensions; body scrolls and active action stays reachable.
Tiny screens may use the full available area with abbreviated decoration.
Closing discards only that layer's transient state; parent form/draft survives.
Outside click is opt-in dismissal and must not confirm or activate background.

Confirmation, text prompt, choice, alert/help and inspection dialogs reuse
`review`, `editor`, `picker` and `content`. Bodies neither start loops nor call
global dialog services. Stack replacement/clear reports dismissal for each owned
interaction and invalidates its asynchronous generation. No callback survives a
destroyed dialog unnoticed. Local failure remains in the dialog with corrective
host action when available, rather than unexpectedly exiting the entire screen.

## Implementation design

### Official dialog design

Every view uses one centered panel. The placement and behavior contract below
governs implementation. Passing engineering checks do not establish human acceptance.

- Every view uses a centered panel, width min(72, viewport width minus four) and viewport height minus
  four rows. Its lighter surface has one column of padding on each side.
- Chat hosts open an alternate screen, redraw the visible conversation tail there,
  and return on close; scrollback stays untouched. This transition is pending the
  inline terminal adapter. The full-screen gallery already owns an alternate screen.
- Height follows content. Prompt fields grow from one to six rows before scrolling.
  No padding rows to a fixed preferred height.
- Shared chrome contains a task title, optional scope at right, an optional
  one-to-three-row message, content and a footer with host-supplied action words.
  Editors use terminal-background insets inside the filled surface.
- Only the top dialog is visible. A nested opening replaces the parent visually
  and shows a title trail; Escape resumes the preserved parent. Replacement adds
  no trail entry. Clear returns to the original background focus.
- Visible background rows repaint faded, including the composer, while drafts
  and cursor state survive. Dark RGB fading uses `#5E6064`, distinct from ordinary
  secondary text; indexed mode uses color 240 while live grey uses 245.
  The visible activity row remains live: warning-colored dialog
  status and normal grey hints identify Escape behavior or nesting depth. A tall
  panel may cover it; the activity row never paints over dialog content.
- An arrival within one second of typing waits instead of taking focus. The
  activity row advertises `! dialog waiting · <subject> · ctrl+g opens it`; Ctrl-G opens it explicitly.
  Timing must be an explicit host input, not a hidden component clock.
- Host removal restores focus and displays the reason for three seconds on the
  explicit host clock. Ctrl-C/Ctrl-D are intercepted above the body: the first
  deliberate press arms closure and the second closes only the top dialog.
  Neither press quits the application. Releases/repeats cannot confirm or close.
- ANSI 16 and no-color modes use a full box and the editor's own field rules;
  indexed/RGB modes use the filled surface. Narrow layouts wrap messages, drop
  scope metadata and shorten footer text while keeping keys reachable.
  Box title/footer lines leave a space next to each corner. Boxed prompt scroll
  hints belong only to the editor's line. In no-color the dialog title is bold
  only; reverse emphasis belongs to the focused field title. The background is
  left plain. ANSI 16 live hints use default foreground plus faint.

Design fixtures are prompt, multiline prompt, danger confirmation, notice,
nested, replaced, waiting, removed, cleared, scrolling, narrow and ANSI 16.
The gallery header, fixture controls and background test editors are host UI.
The gallery separates `late` stale-result rejection from `waiting` arrival behavior.
Its scenarios include replacement/clear/removal, waiting/queued/guarded/unfocused
arrival, close arming, locked Escape, outside presses, field errors, filter clearing,
tiny/tall/ASCII/light rendering, irreversible arming/expiry, exact-name confirmation
and letters typing in fields. The common profile scenarios exercise ANSI 16,
indexed/RGB, NO_COLOR and narrow layouts.

### Arrival, guards and confirmation

The host supplies elapsed time and terminal focus explicitly. An asynchronous
arrival joins a FIFO queue of at most sixteen requests, preserving the host's order.
Duplicate waiting/open IDs and invalid target orders reject atomically. A queued root
opens only when no dialog is active and there has been a one-second pause after
typing or paste. Ctrl-G opens the first waiting root immediately. Explicit nesting
is separate from the arrival queue and preserves the parent's body and focus.
For 300 ms after opening or replacement, Enter and number shortcuts are consumed
without confirming. Other printable keys still type into focused fields.

Opening while the terminal is unfocused emits an identified attention request;
the host owns OSC 9 notification and the `! canonic · needs you` title. Rendering
does not issue notifications. Returning focus clears the waiting-for-you label.
Replacement names the old title in one bounded notice. Host removal displays
`dialog closed · <reason>` for three seconds, restores focus, and invalidates the
opening ticket. A child removal leaves the parent intact. Clear returns all bodies
for cleanup and restores the original surviving base target.

A locked dialog offers an explicit safe No/Deny choice. Escape first clears a
body selection/filter; if nothing consumes it, the activity says
`! answer to continue`. App-level armed closure remains distinct from approval.
Failure updates message/footer in place, using the error role while retaining input.

Caution is host supplied. A normal action confirms on Enter; a destructive but
undoable action has an error label and cannot be the default. For a destructive
irreversible action, the first Enter arms that stable action ID for three seconds;
a second deliberate Enter in that window confirms. Expiry, moving to another
choice, Escape, replacement or changed details disarms. A number shortcut only
moves to a danger choice and never arms or confirms it. A large permanent action
instead requires exact matching of a host-supplied name, without trimming or
case folding. Paste can populate a field but never confirms. Repeated key events
cannot count as a second confirmation. These policies produce intents only;
execution and authority stay with the host.

`Stack::new_timed` enables the arrival policies. Hosts call `typed` after actual
background typing/paste, `tick` with elapsed time and `set_terminal_focused` with
focus observations. `arrive` opens or queues a root; `push` explicitly nests.
`take_attention` consumes notification requests once and excludes closed openings.
`activity` supplies the live row; hosts redraw it only outside the panel.
`remove`, `replace` and `clear_with_reason` produce bounded three-second notices.
Clear returns active bodies top to bottom, followed by unopened arrival bodies in
host order, for host cleanup. Unopened dismissal tickets cannot accept updates.
`withdraw_waiting` returns an unopened request by ID without disturbing focus.
`update_view` keeps the active body and opening while changing failure labels.
The lower-level `Stack::new` retains untimed dispatch for hosts that own these
policies themselves. `Confirmation` owns only local qualification and injected
expiry; a stable action identity includes its host revision. Hosts must disarm it
when selection/details change and emit their own identified intent after Confirm.

## Host boundary

The host owns body components, focus targets, supplied size/close policy and
any asynchronous work. Dismissal records tell it which work to cancel. Bodies
own editing and scrolling; the stack owns only modal routing and chrome.
Host clock input is required for arrival, guard, confirmation and notice behavior.
The host provides safe No/Deny choices for locked bodies and enforces its authority
when consuming results. Notification, OSC/title I/O and terminal leases remain external.

## Acceptance criteria

- **D-01:** Dialog stack handles only top input, restores valid focus, cancels
  owned generations and remains operable at tiny sizes and after resize.
- **DG-02:** Arrival order, one-second pause, Ctrl-G and the 300 ms guard use
  injected time and preserve the background draft/cursor/selection.
- **DG-03:** App Ctrl-C/Ctrl-D never reach the body or quit while a dialog is open;
  deliberate second press closes exactly one layer.
- **DG-04:** Replacement/removal/clear invalidate results, restore surviving focus
  and display bounded timed notices; unfocused attention is an explicit host intent.
- **DG-05:** Destructive confirmation requires deliberate Enter according to the
  host's caution policy; numbers, paste, search and rendering never approve.

Use nested, replaced, cleared, removed-return, scrolling, narrow and color-profile
fixtures. Host timing and inline terminal transitions require separate acceptance.

## Remaining gaps

Native inline chat alternate-screen transitions belong to terminal hosting. Fallback
visual review remains pending in [Gate 2b](../human-testing.md#gate-2b--dialogs).
