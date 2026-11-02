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

This sketch uses consumer-supplied runtime functions such as `spawn_tool` and
`write`; it is not a standalone server. The [Tasks example](../../examples/tasks.rs)
provides a complete server with a consumer-owned store.

```rust,ignore
use canonic_mcp_kit::{Inbound, Session, SessionOptions};

let mut session = Session::new(catalog, SessionOptions::default());

for frame in frames {
    match session.receive(&frame) {
        Inbound::Reply(reply) => write(&reply),
        Inbound::Subscription { acknowledged, .. } => write(&acknowledged),
        Inbound::Fatal { reply, .. } => { write(&reply); break; }
        Inbound::Initialize { id, client } => {
            write(&session.accept_initialize(id, &server_info(&client)));
        }
        Inbound::Tool { id, call } => spawn_tool(id, call),
        Inbound::Task { id, request } => spawn_task(id, request),
        Inbound::Elicitation { id, answer } => resume_rich_input(id, answer),
        Inbound::BundledResource { id, read, .. } => spawn_bundled_read(id, read),
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
- Call `session.finish(&id)` before writing the reply to a tool call, task,
  resource read or extension request, and write it only when `finish` returns `true`. After
  `Inbound::Cancel` it returns `false`, so a cancelled request gets no reply.
- Write frames from one place so they never interleave.
- Use `session.request_confirmation(message)` to start a confirmation: send the
  returned request and wait for `Inbound::Confirmation` with its ID. Use
  `cancel_confirmation` to withdraw it and send the returned notice.
- Call `session.close()` when the input ends and send the notice it returns.
- With [subscriptions](subscriptions.md), drive the nearest subscription
  deadline and send expiry results; gracefully end streams before closing the
  session. Their acknowledgement does not release the original RPC ID.
- Deadlines, lanes, keys and end-of-input handling are yours. The
  [stdio guide](stdio.md) describes the driver's
  choices if you want to match them.

For legacy tool calls, capture `Catalog::result_schema(&call.name)` at admission
and use `ResultSchema::encode_legacy` to validate and encode the result within
an output limit. Modern calls capture their contract in `RequestContext`;
use its typed `encode_success`. Both paths observe the supplied interruption
check. Low-level `rpc::encode_success` only builds an envelope and cannot infer
the producing tool's schema. See [schema validation](schemas.md).

For modern failures, use `RequestContext::encode_error` with the captured
context. It enforces protocol error allocations and required error data within
your output budget. Invalid reserved codes or malformed protocol error data
produce `-32603` without the original data. Application codes belong outside
the JSON-RPC reserved range `-32768..=-32000`; existing legacy server/deadline
allocations retain their meanings. The exact legacy resource-not-found helper
maps to `-32602` only for resource reads.

Modern `resources/read` success requires a typed `ReadResourceResult`.
Use `Outcome::Resource` in the shared driver or `encode_success` with that type
in your runtime. Custom RPCs can use plain JSON objects; a raw object cannot
stand in for a standard tool or resource result. Low-level `rpc::failure` and
`rpc::encode_failure` only build envelopes and do not enforce revision policy.

Optional [Tasks](tasks.md) adds typed management events and
`RequestContext::encode_task`. Capture the request context before settling its
ID. Persistent task lifetime belongs to the consumer, separately from the
RPC ID; cancelling a poll does not cancel its job.
