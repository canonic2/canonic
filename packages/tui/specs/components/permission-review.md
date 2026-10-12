# Permission Review

## Purpose and current status

Present an identified host operation and its supplied choices above Chat's fixed
Composer dock. Choosing and confirming are separate. No opening, navigation,
inspection, search, paste or dismissal authorizes an operation. The host owns
execution, policy, grants, redaction, request lifetime and durable storage.

The reusable component, shared review queue, Chat receipts and 56 deterministic
gallery scenarios are implemented. Engineering evidence belongs to
[Verification](../verification.md); human acceptance is independent.
Related contracts: [Question Form](question-form.md), [Dialog](dialog.md),
[Tool Call Card](tool-call-card.md), [Chat](chat.md), [Editor](editor.md) and
[Activity](../features/activity.md).

## Host descriptors and limits

`Request` carries a nonempty queue-unique ID, increasing material revision, run
label, title, operation summary, reason, ordered resources and safe details.
Resources have a name and host-supplied count/detail. Details reuse tool output
types: text, code, diff or unavailable. The host supplies masked environment
values and safe source; the UI cannot redact secrets it has never received.

Each `Choice` has an opaque ID, visible label, short scope label, exact scope
string, kind (Allow/Deny/Skip), optional disabled reason, broader-scope flag and
destructive flag. IDs are unique within the request. Exact scopes may contain LF
but no other controls; display and response preserve every word. Broader-scope
choices must supply a nonempty scope and be affirmative. A default must name an
enabled, non-destructive Deny or Skip. An affirmative default is rejected, not
silently selected. Every material replacement starts with no selection even if
its descriptor carries a safe default.

Validation is atomic: 1–32 choices, at most 128 resources, at most 64 KiB total
metadata, 4 KiB per exact scope and 256 KiB retained details. Empty/duplicate IDs,
invalid defaults, inconsistent scope kinds or excessive descriptors are rejected
whole. No silently shortened operation reaches the host. Search uses an Editor
bounded to 4 KiB with whole insertion rejection. Rendering sanitizes controls,
wraps whole graphemes and never writes terminal control sequences.

## Behavior and state transitions

| State | Input and transition |
| --- | --- |
| Open | No selection unless a safe host default exists; choose, inspect or dismiss |
| Scope check | Enter on a broader choice opens a Dialog quoting its exact scope; Back is selected |
| Armed | Destructive Enter arms for three seconds; a second unmodified Enter press sends |
| Sending | One immutable response is outstanding; all input locks including Escape |
| Applied | Actual host acknowledgement folds to allowed (with exact scope), denied or skipped |
| Not applied | Host reason visible, selection retained; only R retries the failed attempt |
| Refused | Host reason disables the attempted choice; selection and confirmation clear |
| Changed | Higher material revision clears selection, search, details and outstanding tickets |
| Expired / withdrawn | Choices disabled, reason visible; details remain readable; Escape closes |
| Dismissed | Escape without a response; distinct from Deny/Skip and never affirmative |

Only a matching opaque single-use ticket can consume an acknowledgement. Foreign,
stale and duplicate results are inert. Host expiry/withdrawal invalidates a sent
attempt immediately; a late Applied acknowledgement cannot resurrect it. Sending
counts elapsed seconds after three seconds and never times out automatically.
Failure does not silently resend on Enter, even after details/search. R resends
the same immutable ID/revision/choice/scope with a fresh ticket and incremented
retry count. Choosing a different response starts a new confirmation sequence.
Refusal is scoped to this material revision; replacement may supply new choices.

The component never stores a grant. A later covered-operation tool card is a
host-supplied fact. Allowed receipts reproduce the acknowledged scope exactly;
denied, skipped, dismissed, expired and withdrawn receipts remain distinct.

## Keyboard and focus

| Key | Effect |
| --- | --- |
| Up/Down, Home/End | Navigate details row and choices; enabled and disabled rows can be inspected |
| 1–9 | Highlight a supplied choice, never confirm; later rows remain reachable by navigation |
| Enter | Toggle the highlighted details row or confirm a live enabled choice |
| Tab | Show/hide details without sending or changing scope |
| PageUp/PageDown | Scroll open details; scope check scrolls its full exact wording |
| Ctrl-F | Open literal case-insensitive loaded-details search |
| Enter in search | Next match, never a permission response |
| R after failure | Explicit retry, same choice and material revision |
| Escape | End search → hide details → dismiss; in the scope Dialog return to choices |
| Ctrl-G in Chat | Open a waiting first review immediately |
| Paste | Drop the complete paste in every review scope, including search and Dialog |

The dropped-paste notice is `paste ignored · a review takes no text`. Repeat and
release events cannot confirm, retry or dismiss; repeats may move/scroll. Modified
Enter never confirms. In the scope check, numbers only highlight Back/Allow;
Down or 2 then Enter is required to confirm. Escape or Enter on Back returns
without a response and retains the wider choice. Movement, details/search changes,
Escape, replacement and expiry disarm destructive confirmation. If a choice is
both broader and destructive, the second check also requires two deliberate
Enter presses. Paste never edits the background Composer or search query.

## Placement, rendering and profiles

The title rule shows operation, run/revision/lifecycle and +N waiting. Summary
is bold, reason muted. First four resources show host counts; additional resources
are counted and all appear in scrollable details. Diff inspection reuses Tool
Call Card source classification, line numbers and semantic add/delete roles.
Commands and requests show host-supplied safe source. Details scroll internally;
choices and footer stay outside that viewport. Long choice lists scroll to the
cursor with omitted counts. Search reveals matching source without responding.

At widths below 50, scope labels wrap below choices and hide while details are
open; details omit line numbers and wrap with continuation markers. The broader
scope Dialog wraps without shortening and scrolls when necessary. A one-cell
frame remains safe; compact hints prioritize Enter and Escape. Rules and markers
adapt independently to ASCII. NO_COLOR uses reverse selection, brackets around
scope and words for every outcome. ANSI16 preserves the existing selection
policy while its reverse-versus-cyan decision remains open. Light uses the same
semantic layout. Injected-clock feedback has no required animation, so reduced
motion retains all elapsed/status meaning.

## Shared queue and Chat contract

`ReviewQueue` owns one active Question Form or Permission Review and at most 16
later requests in explicit host order. Stable IDs are unique across both types.
Arrivals while actual Composer text changes wait for one second of quiet or
Ctrl-G. Cursor movement alone does not postpone opening. Later arrivals never
replace active focus. Host reordering must list every pending ID exactly once,
and retains the active request. Material replacement addresses the same identity
with a higher revision and invalidates old work. Waiting withdrawal removes the
entry; active Permission withdrawal keeps the locked explanation until Escape,
then opens the next review. No response crosses a request or component instance.

Chat keeps the same Composer instance: document, caret, selection, mode, undo
and pending local overlays stay intact. Review input consumes events before
background scopes. Logical transcript anchor/follow state is saved on opening
and restored on closure; output arriving during review remains unread. Receipts
use host acknowledgement rather than optimistic approval. One terminal owner
supplies frames/events/elapsed time; components acquire no terminal and run no
event loop. Hosts drain identified intents and closed panels regularly.

## Implementation design and host boundary

`permission_review.rs` owns the validated request, cursor, lifecycle, immutable
attempt/receipt and opaque tickets. Model validates descriptors; input owns
review/search/Dialog routing and reusable Dialog destructive confirmation; view
owns summary/resources/choices and safe full inspection. `review_queue.rs` owns
mixed ordering/timing and queues identified intents. Chat adapts closed panels to
receipts and saved viewport restoration. Gallery owns simulated acknowledgements,
expiry, replacement and elapsed time, with no agent execution or grant writes.

Public construction and material replacement return atomic errors. Hosts invoke
`acknowledge`, `expire` or `withdraw`; an acknowledgement is evidence of what
the host actually applied, never permission enforcement by this library.
`render_with` draws a caller-owned panel; `render_overlay` draws its scope Dialog
in a separate caller-owned viewport. Standalone hosts route every active review
event before other UI and drain effects once.

## Acceptance criteria

- **PR-01:** Every affirmative operation requires highlighted choice and deliberate
  Enter; safe defaults, numbers, repeat/release, paste, inspection and dismissal
  cannot grant permission.
- **PR-02:** Broad scope quotes/returns exact wording and starts on Back;
  destructive confirmation expires/disarms and binds current material.
- **PR-03:** Pending locks, exact tickets, explicit retry, refusal, replacement,
  expiry and withdrawal reject stale/cross-instance completions.
- **PR-04:** Shared queue preserves host order and active focus; Chat restores
  exact draft/caret/selection/undo, anchors/follow and concurrent unread output.
- **PR-05:** All resources/details remain inspectable, search never sends, and
  narrow/tiny/ASCII/ANSI16/NO_COLOR/light/reduced-motion states remain readable.
- **PR-06:** Gallery exercises all lifecycle/confirmation/queue states with
  deterministic host acknowledgements; behavioral/render/native tests provide
  engineering evidence separately from human/platform acceptance.

## Remaining gaps

Human visual acceptance, broader terminal/platform acceptance and real consumer
adapters remain separate. Permission policy, grants, durable history, agent
execution and noninteractive fallback are host-owned. Session Browser and general
standalone code/diff inspection are separate components.
