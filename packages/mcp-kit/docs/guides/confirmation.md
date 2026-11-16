# Ask the user to confirm

This guide covers the initialization-based protocol. The opt-in modern path
uses [consumer-owned confirmation retries](skills.md#modern-confirmation-retries).

Some actions need the person using the host, not the agent, to agree. A
handler returns `Outcome::Confirm`; the host shows the message as a form with
Accept and Decline, which the user can also dismiss. Your continuation receives
the reported outcome.

```rust,ignore
use canonic_mcp_kit::{Decision, stdio::{Confirm, Outcome}};

fn call(&self, request: Request, context: &Context) -> Outcome {
    let plan = self.prepare_removal(&request); // nothing changes yet
    Outcome::Confirm(Confirm::new(
        format!("Remove the Acme service {} and its data?", plan.service),
        move |decision, context| match decision {
            Decision::Accepted => plan.apply(context), // recheck the plan, then act
            Decision::Declined | Decision::Cancelled => declined("The service was kept."),
            Decision::Unavailable => declined(
                "This host cannot ask for confirmation. Ask the user to run the removal in a terminal.",
            ),
        },
    ))
}
```

## What to rely on

- Only `Decision::Accepted` means the host reported that its user accepted.
  Treat everything else as no.
- `Declined` means an explicit refusal; `Cancelled` means the user dismissed
  the form without deciding. A host reply with a missing or unknown action
  yields `Unavailable`, so offer an alternative such as running the command
  in a terminal. Malformed modern confirmation retries instead return an
  invalid-params error.
- The host must declare form elicitation. Otherwise the continuation runs
  with `Unavailable` and no form is shown. Tell the agent what the user can do instead.
- While another legacy question is pending, the driver returns RPC error
  `-32000` with `data.code: "human_input_busy"` and its originating `requestId`.
  Answer that question or cancel its request with `notifications/cancelled`,
  then retry. The competing continuation does not run and the original stays live.
  Independent runtimes can inspect `Session::pending_question()` to distinguish
  a busy slot from unsupported input.
- If the encoded form exceeds `Options::max_output_bytes`, no form is sent
  and the continuation receives `Unavailable`.
- The worker is free while the user decides. The question has no response
  deadline while the request and connection remain live. Human waiting pauses
  the active request budget; the continuation receives only the time left.
- The continuation keeps the request's key, so no other request with that key
  runs between the question and the answer. State can still change through
  other means: recheck what you are about to change before acting.
- The continuation never runs when the request is cancelled or the host
  disconnects. Late or duplicate answers are ignored.
- A continuation cannot ask a second time.

The library does not authenticate the user or the host. A confirmation is the
host's report of its user's choice; your server decides whether that is
enough for the action.

Client capability declarations do not prove that the prompt is visible. The
driver relies on the live pending request and connection, and stops waiting on
explicit cancellation or disconnect. A host may impose its own deadline;
report that boundary and offer a fresh review when needed. Before effects,
revalidate the exact proposal and current permissions. If it materially changed,
obtain a new decision rather than applying the old answer to different work.
