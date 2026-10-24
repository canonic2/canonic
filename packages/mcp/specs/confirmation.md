# Confirmation

A server asks the host's user before an action by sending a form
`elicitation/create` request. The library carries the question and the answer;
it never decides on the user's behalf, and the server must not act on
anything but an accepted answer.

## Session rules

- Confirmation is available only when the client declared form elicitation:
  `capabilities.elicitation` is an empty object or has a `form` object.
- At most one confirmation is pending per connection. Another request while
  one is pending, a client without form elicitation and a closed session all
  report `Decision::Unavailable` without sending anything.
- The request carries the message and an empty object schema, so the form has
  only Accept, Decline and dismissal. With protocol `2025-11-25` it also
  carries `mode: "form"`; with `2025-06-18` it does not.
- Server request IDs are `confirmation-<n>`, separate from client IDs.
- `action: "accept"` is `Accepted`. Any other result action, including
  `decline` and `cancel`, is `Declined`. An error response or a malformed
  response is `Unavailable`. A response to any other ID is ignored.
- Withdrawing a pending confirmation sends `notifications/cancelled` for its
  ID; a later answer to it is ignored.

## Driver rules

- A handler returns `Outcome::Confirm` with a message and a continuation. The
  worker is released while the user decides; the confirmation has its own
  deadline.
- On an answer, the continuation is queued at the front of its lane with a
  fresh request deadline and receives the decision.
- When confirmation is unavailable, the continuation is queued at once with
  `Unavailable`, so the server can report that the user could not be asked.
- A confirmation request that exceeds the encoded output limit is not sent.
  Its pending state is cleared and its continuation is queued with `Unavailable`.
- The continuation does not run when its request is cancelled, its
  confirmation deadline passes or the input ends. The pending form is
  withdrawn; a deadline is answered with `-32001` "Confirmation deadline
  exceeded" and end of input with `-32800`. An answer read after the
  confirmation deadline gets the same `-32001` reply, even before the next
  deadline sweep, and does not resume. A confirmation asked for after the
  input ended is answered with `-32800` without sending a form, and its
  request's key is released.
- A continuation that asks for confirmation again is answered with `-32603`.
- A notification cannot ask for confirmation; its outcome is discarded.
