# Activity, review and cancellation

Status: [Tool Call Card](../components/tool-call-card.md) implements reusable
identified activity/output, ordered concurrency, preview expansion and acknowledged
copy/details/retry through Chat. [Run Activity](../components/run-activity.md)
implements run progress, cancellation races and independent connection state.
[Question Form](../components/question-form.md) and
[Permission Review](../components/permission-review.md) implement isolated human
reviews above the Composer, sharing one host-ordered queue. Gallery effects are
deterministic simulations; authority remains host-owned.

## Tool and run activity

Host supplies run ID/revision and explicit state: idle, preparing, running,
waiting for input, cancelling, cancelled, completed, interrupted, failed or disconnected.
Connection and run state are independent: a completed tool error does not imply
connection loss. Progress is determinate only when host supplies meaningful
totals; otherwise show labeled activity/elapsed time. Unknown completion stays
unknown. Animation is optional, uses supplied time and stops on leaving/dismissal.

The [card contract](../components/tool-call-card.md) owns exact call states,
rendering limits, transitions, selected keys and host acknowledgements.
Reusable tool cards accept stable call ID, revision, verb/target, safe unknown-tool
argument summary, lifecycle, supplied elapsed duration and text/code/diff output
or an unavailable reason. The host summary supplies result/error interpretation;
related artifact routing belongs to host composition.
States distinguish queued/running/waiting/completed/failed/cancelled;
absence of a result is not success. Redaction happens before the UI boundary;
the library cannot infer which arguments contain credentials. It additionally
sanitizes controls and bounds rendering. A missing/unknown tool gets a generic
card with the supplied label, state and available details.

Collapsed cards show state and a bounded useful summary. Explicit expand opens
detail without stealing the draft. Large output carries omitted counts/marker
and inspection/paging intent; collapsed display does not destroy host output.
Host-specialized bodies reuse [Content](../components/chat.md) contracts for shell
output, reads, edits, searches, diagnostics and diffs. No tool-name switch that
embeds an agent-specific argument schema belongs in the generic library.

Concurrent cards retain start order, identities, expansion and anchor through updates.
Child task activity can refer to a supplied session/tree node and return open
intent; orchestration and routing are host-owned. Failures display useful safe
causes; retry returns an identified intent only if host enables it.

## Cancellation contract

Interrupt emits a run/revision-scoped intent with an opaque attempt ticket once
while pending. Chat additionally identifies the conversation. Preserve
draft and transcript. Show cancelling until host acknowledges interruption,
failure or completion. If completion wins the race, show completed and clear
pending cancel; do not rewrite it as interrupted. If host reports cancellation
timeout/error, display that result and permitted retry/exit actions. A later
run does not inherit an earlier cancellation request.

UI cancellation ends UI-owned transient interaction; it does not prove process
termination. Agent adapter owns interrupts, deadlines, reconnect and descendants.
Leaving a screen invalidates its fixture work; real host background operations
may continue according to host policy. UI never kills a product process.

## Permission and confirmation workflow

Implemented [Permission Review](../components/permission-review.md) accepts request ID/revision, affected run, title, operation summary,
safe details/diff, choices with opaque response IDs and explicit scope labels,
safe default or no selection, and expiry/pending state.

1. Opening saves focus and preserves draft; pending requests are identified and
   can queue without silently replacing one being reviewed.
2. User navigates, inspects/expands detail and selects an enabled response.
   Ordinary highlighting, search, paste and closing never approve.
3. Confirm emits the exact current request/revision/choice. UI shows pending
   response and suppresses duplicate Enter/repeat. Host checks authority and
   applies or refuses it; only its acknowledgement establishes completion.
4. Updated material details invalidate staged confirmation. Expired/withdrawn
   requests disable responses and show why. An unrelated request cannot receive
   an old response. Failed application is visible and retry requires explicit action.

Caller may supply allow-once, deny, skip or broader-scope choices. The UI does not
invent a grant scope or store it. Broader grants use an additional review of exact
host-supplied scope when the host requires it. Denial, cancellation/dismissal and
skip are distinct response IDs/intent kinds. Escape returns a distinct dismissal
intent without choosing a negative or affirmative response.
Host handles noninteractive fallback without acquiring a UI lease.

## Questions

Implemented [Question Form](../components/question-form.md) supports one/multiple questions with stable IDs, description,
single/multiple choice, disabled options, optional typed answer, required/min/max
constraints and staged answers. Navigation between questions preserves answers.
Multiple-choice toggling is separate from final submission. Review lists answers
and missing required fields; final submit returns one versioned response envelope.
Typed-answer editor reuses `editor`, with cancel returning to choices and retaining
the staged answer unless explicitly cleared. Validation is local presentation
validation; host still validates authoritative question semantics.

Requests becoming stale during typing preserve unsent text for inspection but
cannot submit it to the replacement request. Form cancellation emits no partial
submission unless host explicitly provides partial-response capability. Queued
questions/permissions are shown in host-defined order; UI does not rank policy.

## Gallery isolation

Fixtures include parallel tool calls, growing shell output, read/search summary,
code/diff edits, unknown tool, failed tool with live connection, disconnected run,
cancel/completion race, no acknowledgement, expired/changed review, queued reviews,
multi-question/typed answer and response application failure. All operations are
simulated. Permission fixtures must show that selecting a response changes only
fixture state, with no grant file or product call.

## Acceptance criteria

- **R-01:** Concurrent/unknown/growing/failed cards maintain identity and readable
  bounded details; run/connection/progress states do not imply fabricated success.
- **R-02:** Cancellation emits once for the right run, preserves draft and resolves
  completion/interrupt/error races from host acknowledgements.
- **R-03:** Review never approves on open/default movement/paste/close; responses
  bind exact ID/revision, suppress duplicates and invalidate on change/expiry.
- **R-04:** Single/multi/typed question flow preserves staged answers, validates
  constraints, exposes review and emits no accidental partial response.
- **R-05:** Modal/queued/stale/error review states preserve background draft and
  valid return focus; gallery produces no authority or product side effects.

## Review placement

Question and permission panels sit above the dock. The Composer dims while a
panel owns focus and retains its full draft, cursor, selection and scroll.
Requests arriving during typing remain in the activity row until a one-second
pause or Ctrl-G. Their shared queue follows host order and shows additional
waiting requests. The reusable shared queue and Chat composition implement this
local behavior; hosts own execution, authority, storage and request lifetime.

The host owns request order and authority. Permission dismissal is a distinct
outcome from Deny/Skip; wider-scope choices require another confirmation quoting
the host's exact scope, introduced by the host's own sentence when it supplies
one, wrapped at word boundaries and set apart in bold when the terminal has no
surface color. Single-question answers submit explicitly on choosing;
multi-question answers pass through review, with required omissions shown there.
