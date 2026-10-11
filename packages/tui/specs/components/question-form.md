# Question Form

## Purpose and current status

Present host-supplied single-choice, multiple-choice and typed questions above
Chat's Composer. Stage answers locally and send one identified response only
after deliberate confirmation. Execution, request lifetime, transport, storage,
acceptance and review ordering belong to the host.

This is the complete intended local contract. Implementation and engineering
evidence are pending. Human visual and platform acceptance remain independent.
Related contracts: [Chat](chat.md), [Editor](editor.md), [Dialog](dialog.md) and
[Activity](../features/activity.md).

## Host inputs and identities

A request has a stable host ID, monotonically increasing material revision,
title, ordered questions and an explicit partial-answer capability. Each question
has a stable ID, short step label, prompt, optional description, kind, required
flag, choice IDs/labels/details/disabled reasons and applicable limits. The host
may supply a highlighted single-choice default; highlighting never stages it.
A typed answer can be its own question or an explicit last Something else row.

Reject invalid snapshots atomically: empty/duplicate IDs, invalid defaults,
inverted mark bounds, an empty choice-only question or excessive descriptors.
Bound requests to 32 questions, 128 choices per question and 64 KiB of descriptor
text. Number shortcuts address the first nine choices/questions; navigation
reaches the remainder. Fields use Editor's 64 KiB transaction limit. An insert
that would exceed that resource limit is rejected whole with a visible reason.
The host's character limit is validation only: retain over-limit text and show
its count. Count Unicode scalar values consistently, including newlines.

An opaque response ticket identifies one immutable snapshot of answers and its
request/revision. Copies and acknowledgements use their own single-use tickets.
Foreign, duplicate, replaced and expired completions have no effect. Replacing
a material revision invalidates every old ticket, clears choices and fields,
and retains the old answer text solely for an explicit copy request.

## Local state and transitions

| State | Input and resulting behavior |
| --- | --- |
| Waiting | Arrival during typing leaves Composer focused. One second without typing opens it; Ctrl-G opens immediately. Further typing restarts the pause. |
| Answering | Choice highlights, marks and field text are local. Navigating retains every answer. |
| Review | Two or more questions list all answers and missing required answers. Enter validates the whole response. |
| Submitting | Emit one response ticket; dim and lock the form. Repeated Enter, typing, paste and cancellation do nothing until the host responds or withdraws it. |
| Accepted | Only an exact host acknowledgement closes the form. The host projects a receipt with one line per supplied answer; skipped answers are omitted. |
| Failed | Reopen the same step/review with all answers intact and an error reason. Another deliberate Enter creates a fresh response ticket. |
| Refused | Reopen the identified question with staged answers and the host's error reason. The host can refuse locally valid answers. |
| Cancel confirmation | With answers, open a Dialog defaulting to Keep answering. Esc keeps. Without answers, close immediately and emit Cancelled, never a submission. |
| Partial submitting | Only an offered and explicitly chosen Send answers so far action emits a partial response. Include answered questions only and label the eventual receipt partial. |
| Expired | Disable changes/submission, keep answers and warning visible. Y requests copy; Esc closes without sending. |
| Replaced | Fresh material request opens with no inherited answers. Y can copy the retained old text; no old response can complete the new opening. |

A single question has no step row or review: choosing a valid single answer or
confirming marks/text submits immediately. Multiple questions always require
Enter on Review. Selecting a single choice advances; Space marks multiple
choices without advancing. Returned choice IDs follow the host's source order.
An optional empty question is skipped. Required text must contain non-whitespace
content. Multiple-choice validation explains minimum and maximum mark bounds.
At the maximum, unmarked choices fade but remain reachable; trying to mark one
shows why. Disabled choices remain reachable to read their reason and cannot
stage an answer. Choosing an enabled alternate replaces a staged typed answer.

## Keyboard and focus

| Input | Behavior |
| --- | --- |
| Up/Down | Move through choices, including disabled ones. Long lists scroll locally. In an active field these are Editor keys. |
| 1–9 | On single choice, choose and advance/submit. On Review, jump to edit that question. Enter after editing returns directly to Review. |
| Space | Toggle a multiple-choice mark; never submit. |
| Enter | Choose, validate marks, finish text or send Review. Missing Review answers focus the first invalid question. |
| Tab/Shift-Tab | Next/previous step, even if required content is missing. At either end stay there. |
| Left/Right | Previous/next step outside a field; within a field move the caret. |
| Ctrl-J | Insert a field newline. |
| Esc | Field to staged choices; form to cancellation; confirmation to Keep; expired to Close. |
| Delete | On a staged typed row, clear it as one undoable operation. Ctrl-Z restores it. In a focused field use normal Editor deletion. |
| Y | Copy retained expired/replaced answers through an identified host intent. |
| Ctrl-G | Open a waiting request; in a field retain Editor's select-all behavior. |

Typing the first letter or pasting while the Something else row is highlighted
opens its field and inserts that input. Paste never confirms. Field editing,
selection, undo and all form navigation consume input before Composer sees it.
Preserve Composer's exact document, caret, selection, mode and undo state. Save
Chat's logical viewport/follow state on opening and restore it on closure;
concurrent transcript updates remain anchored and unread.

## Rendering and composition

Use a title rule with right-aligned step metadata, a step row for multiple
questions, a bold wrapped prompt, muted description, choices/field, one validation
or host-status line and a footer rule with counts. Steps use answered, current,
not-yet, missing and skipped signs. Single choice uses numbers and a chosen mark;
multiple choice uses marks. Details and disabled reasons are muted; the current
row uses selection. Fields grow from one to six rows and then scroll internally.

Chat reserves the panel directly above the fixed dock and leaves at least three
transcript rows when geometry permits. Dim Composer while retaining its visible
draft. The shared activity/footer display form status and applicable keys.
Cancel confirmation overlays the same owned terminal through Dialog; it never
acquires a new session. Later reviews count as +N waiting and open in the host's
order after the current request closes. A queued arrival never replaces it.

Below 50 columns, shorten metadata to 2/3, retain only the current step name,
drop choice details and place review answers under their labels. Fields lose
their indent. Preserve Enter/Esc hints before secondary labels. Tiny geometry
still routes cancellation and confirmation safely; no hidden key becomes an
implicit answer. Long lists expose counts above/below the visible window.

NO_COLOR brackets the current step, reverses the cursor row and uses [x], [ ]
and [-] marks. ASCII independently uses >, ok, o, -, x and plain rules; Unicode
answer content remains unchanged. ANSI16 uses palette roles only; light/dark
use supplied semantic roles. Render safe textual control markers rather than
interpreting terminal controls. Host time drives waiting/submission labels;
reduced motion preserves elapsed seconds and removes cycling decoration.

## Acceptance criteria

- QF-01: single and multi-question flows emit exactly one identified response
  after their respective confirmation points; host refusal retains every answer.
- QF-02: required/min/max/text validation explains the next action; navigation
  never blocks or erases staged answers and returned marks retain source order.
- QF-03: fields isolate paste, multiline editing, selection and undo from the
  exact Composer draft/caret; deleting a staged typed answer is undoable.
- QF-04: cancellation defaults to Keep, sends nothing, and offers partial
  submission only when the host explicitly enables and the user chooses it.
- QF-05: expiry, replacement, stale/foreign acknowledgement, delayed opening
  and host-ordered review queues preserve identity and focus without authority.
- QF-06: all request/form/navigation/typed/fallback/placement states have
  meaningful live gallery fixtures; test narrow, zero-sized, ASCII, ANSI16,
  NO_COLOR and light rendering, real PTY input/resize/return and cleanup.
- QF-07: package checks, release/library-only/consumer compilation, local links,
  ownership and whitespace pass; human visual acceptance is recorded separately.
