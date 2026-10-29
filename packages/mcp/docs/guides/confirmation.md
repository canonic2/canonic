# Ask the user to confirm

This guide covers the initialization-based protocol. The opt-in modern path
uses [consumer-owned confirmation retries](skills.md#modern-confirmation-retries).

Some actions need the person using the host, not the agent, to agree. A
handler returns `Outcome::Confirm`; the host shows the message as a form with
Accept and Decline, and your continuation receives the answer.

```rust,ignore
use canonic_mcp::{Decision, stdio::{Confirm, Outcome}};

fn call(&self, request: Request, context: &Context) -> Outcome {
    let plan = self.prepare_removal(&request); // nothing changes yet
    Outcome::Confirm(Confirm::new(
        format!("Remove the Acme service {} and its data?", plan.service),
        move |decision, context| match decision {
            Decision::Accepted => plan.apply(context), // recheck the plan, then act
            Decision::Declined => declined("The service was kept."),
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
- The host must declare form elicitation. Otherwise, and while another
  confirmation is pending, the continuation runs with `Unavailable` and no
  form is shown. Tell the agent what the user can do instead.
- If the encoded form exceeds `Options::max_output_bytes`, no form is sent
  and the continuation receives `Unavailable`.
- The worker is free while the user decides. The confirmation has its own
  deadline (`Options::confirmation_timeout`, five minutes by default); the
  continuation then gets a fresh request deadline.
- The continuation keeps the request's key, so no other request with that key
  runs between the question and the answer. State can still change through
  other means: recheck what you are about to change before acting.
- The continuation never runs when the request is cancelled, the
  confirmation expires or the host disconnects.
- A continuation cannot ask a second time.

The library does not authenticate the user or the host. A confirmation is the
host's report of its user's choice; your server decides whether that is
enough for the action.
