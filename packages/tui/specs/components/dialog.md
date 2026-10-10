# Dialog

## Purpose

Present caller-owned bodies in an isolated modal stack while preserving parent
state and restoring surviving background focus.

## Current status

Status: stack mechanics and slice 2b.1 visual design are implemented; human visual
acceptance remains pending. Slice 2b.2 typing-aware arrival and timed removal
notices remain unimplemented.

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
Shift/Alt-Enter insert prompt newlines and never confirm a choice or submit a form.

`DialogView` supplies scope, message and normal/compact action footer.
`Stack::render_with` measures body height at the available width before placing
the centered panel. Panels are at most 72 columns and leave four viewport rows.
Tiny viewports omit chrome below three rows. The compatibility
`render` method retains the preferred size height without a body measurement.
Only the top body renders, with parent titles in its trail. The view fades the
already-rendered visible background, fills the surface in indexed/RGB modes,
and uses a full box in ANSI 16/no-color. Body components own scrolling and action
reachability; gallery prompt fields grow to six rows, then scroll. The eleven
gallery scenarios use an editor, picker and local inspection text.
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
governs implementation. The implementation below
covers the visual shell and stack mechanics; it does not establish human acceptance
or implement the timing requirements below.

- Every view uses a centered panel, at most 72 columns and viewport height minus
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
  activity row advertises `! dialog waiting · ctrl+g`; Ctrl-G opens it explicitly.
  Timing must be an explicit host input, not a hidden component clock.
- Host removal restores focus and displays the reason for three seconds on the
  explicit host clock. Ctrl-C remains an application action in every state.
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
The gallery has eleven fixtures covering visual presentation and stack mechanics.
Waiting and timed removal fixtures belong to 2b.2. Existing stack/focus/ticket
tests remain engineering evidence.

## Host boundary

The host owns body components, focus targets, supplied size/close policy and
any asynchronous work. Dismissal records tell it which work to cancel. Bodies
own editing and scrolling; the stack owns only modal routing and chrome.
Host clock input is required for the pending arrival/removal timing behavior.

## Acceptance criteria

- **D-01:** Dialog stack handles only top input, restores valid focus, cancels
  owned generations and remains operable at tiny sizes and after resize.

Use nested, replaced, cleared, removed-return, scrolling, narrow and color-profile
fixtures. Host timing and inline terminal transitions require separate acceptance.

## Remaining gaps

Typing-aware arrival and timed removal notices remain unimplemented. Native
inline chat alternate-screen transitions belong to terminal hosting. Fallback
visual review remains pending in [Gate 2b](../human-testing.md#gate-2b--dialogs).
