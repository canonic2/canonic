# Drive the session yourself

For explicit modern protocol opt-in, per-request reply context and bundled
resource dispatch, see [Skills and modern context](skills.md#protocols-context-and-caching).
Use `Session::try_new` to validate that composition before accepting input.
`Session::new` also validates configuration, but defers reporting failures:
processing a parsed message then returns `Inbound::Fatal` with code `-32603`
and a null request ID. Use `try_new` when startup must report configuration
errors before a client connects.

The stdio driver is synchronous. A server with its own runtime, such as an
asynchronous one, can use the protocol core directly and keep the same
protocol behavior. `Session` performs no I/O and starts no threads.

## The loop

```rust,ignore
use canonic_mcp::{Inbound, Session, SessionOptions};

let mut session = Session::new(catalog, SessionOptions::default());

for frame in frames {
    match session.receive(&frame) {
        Inbound::Reply(reply) => write(&reply),
        Inbound::Fatal { reply, .. } => { write(&reply); break; }
        Inbound::Initialize { id, client } => {
            write(&session.accept_initialize(id, &server_info(&client)));
        }
        Inbound::Tool { id, call } => spawn_tool(id, call),
        Inbound::InvalidArguments { id, error, .. } => {
            if session.finish(&id) { write_tool_error(&id, &error.message); }
        }
        Inbound::Method { id, method, params } => spawn_method(id, method, params),
        Inbound::Notification { method, params } => spawn_notification(method, params),
        Inbound::Cancel { id } => cancel(&id),
        Inbound::Confirmation { id, decision } => resume(&id, decision),
        Inbound::Ignored => {}
    }
}
```

## Rules you take on

- Pass the pending `Inbound::Initialize` ID to `accept_initialize`. An ID
  mismatch or a call without a pending initialization returns `-32600` and
  leaves the session state unchanged.
- Call `session.finish(&id)` before writing the reply to a tool call or
  extension request, and write it only when `finish` returns `true`. After
  `Inbound::Cancel` it returns `false`, so a cancelled request gets no reply.
- Write frames from one place so they never interleave.
- Use `session.request_confirmation(message)` to start a confirmation: send the
  returned request and wait for `Inbound::Confirmation` with its ID. Use
  `cancel_confirmation` to withdraw it and send the returned notice.
- Call `session.close()` when the input ends and send the notice it returns.
- Deadlines, lanes, keys and end-of-input handling are yours. The
  [stdio guide](stdio.md) describes the driver's
  choices if you want to match them.

`rpc::encode_success` encodes a reply directly from a `ToolResult` within an
output limit, and `json::encode_checked` stops encoding when your
cancellation check fails.
