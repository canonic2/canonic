# Activity, review and cancellation

Status: proposed target. Chat currently supplies a status string, plain activity
messages and a generic cancel action; gallery simulates failures/interruption.
Structured activity and pending human requests are separate capabilities.

## Tool and run activity

Host supplies run ID/revision and explicit state: idle, preparing, running,
waiting for input, cancelling, completed, interrupted, failed or disconnected.
Connection and run state are independent: a completed tool error does not imply
connection loss. Progress is determinate only when host supplies meaningful
totals; otherwise show labeled activity/elapsed time. Unknown completion stays
unknown. Animation is optional, uses supplied time and stops on leaving/dismissal.

Tool cards accept stable call ID, title/category, safe argument summary, lifecycle,
optional start/end/elapsed, related artifact IDs, text/code/diff output and error.
States distinguish proposed/queued/running/waiting/completed/failed/cancelled;
absence of a result is not success. Redaction happens before the UI boundary;
the library cannot infer which arguments contain credentials. It additionally
sanitizes controls and bounds rendering. A missing/unknown tool gets a generic
card with the supplied label, state and available details.

Collapsed cards show state and a bounded useful summary. Explicit expand opens
detail without stealing the draft. Large output carries omitted counts/marker
and inspection/paging intent; collapsed display does not destroy host output.
Host-specialized bodies reuse [Content](conversation.md) contracts for shell
output, reads, edits, searches, diagnostics and diffs. No tool-name switch that
embeds an agent-specific argument schema belongs in the generic library.

Concurrent cards retain identities, expansion and anchor through reorder/update.
Child task activity can refer to a supplied session/tree node and return open
intent; orchestration and routing are host-owned. Failures display useful safe
causes; retry returns an identified intent only if host enables it.

## Cancellation contract

Interrupt returns `(conversation, run, revision)` once while pending. Preserve
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

`review` accepts request ID/revision, affected run, title, operation summary,
safe details/diff, choices with opaque response IDs and explicit scope labels,
safe default or no selection, expiry/pending state and close policy.

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
skip are distinct response IDs/intent kinds. Closing may return dismiss or a
caller-defined negative response, explicitly documented; never affirmative.
Host handles noninteractive fallback without acquiring a UI lease.

## Questions

Question form supports one/multiple questions with stable IDs, description,
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

## Unresolved decisions

Choose whether small review panels replace the composer region or open overlays
by default, using narrow-screen evidence. Host chooses request queue priority,
dismiss semantics and grant confirmation policy; library must support those
explicit inputs without selecting product policy.
