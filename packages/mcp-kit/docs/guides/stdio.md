# Serve over stdio

`stdio::serve` reads frames on a reader thread, handles the protocol on the
calling thread and runs your requests on worker threads. This guide covers
the choices you make.
Tool calls must return `Outcome::Tool(ToolResult)`, including when no output
schema is declared; raw `Outcome::Method` success replies are refused.

## Lanes

A lane is a group of workers with its own queue and capacity. The default is
one lane with one worker and room for eight requests, so requests run one at a
time. Give slow reads their own lane so they never wait behind mutations:

```rust,ignore
use canonic_mcp_kit::stdio::{Admission, Lane, Options, Request};

let options = Options {
    lanes: vec![Lane::new("changes", 1, 16), Lane::new("reads", 4, 4)],
    ..Options::default()
};

fn admit(&self, request: &Request) -> Admission {
    match request {
        Request::Tool(call) if call.name == "acme_logs" => Admission { lane: 1, ..Admission::default() },
        _ => Admission::default(),
    }
}
```

A full lane answers new requests with `-32000`. `ping`, `tools/list` and
resource/template listings never wait for a lane. Resource reads use the lane
chosen by `Handler::admit`; see [resources](resources.md).

The dispatcher writes and flushes frames synchronously. If the host stops
reading output, a blocked write also delays ping, cancellation and deadline
processing. Keep admission and diagnostic callbacks quick, keep result sizes
bounded, and use a separate connection when a workload needs independent
output progress.

## Keys

Requests with the same key in one lane run one at a time, in order. Use a key
for the resource a request changes, such as a service name, so changes to one
service are serialized while other services proceed:

```rust,ignore
Admission {
    key: call.arguments["service"].as_str().map(str::to_owned),
    ..Admission::default()
}
```

## Deadlines

Every request has a deadline from admission: `Admission::timeout`, or
`Options::timeout`. When it passes, the host gets `-32001` and the request's
context is cancelled. Check the context at safe points and stop when it fails:

```rust,ignore
fn call(&self, request: Request, context: &Context) -> Outcome {
    for step in steps {
        if context.check().is_err() {
            // After cancellation or the deadline the outcome is discarded; after
            // a disconnection it is still written if the output is open.
            return Outcome::Tool(stopped_result());
        }
        step.run();
    }
    // ...
}
```

The library cannot interrupt code that does not check. Once a request's
deadline reply is sent, its later outcome is discarded, so a mutation that
finishes late may have taken effect; make retries safe or tell callers to
check state first.

## Progress

When a call carries a progress token, `context.progress(done, Some(total),
Some("message"))` sends `notifications/progress`. Progress is best effort: it
is dropped when the buffer is full or after the reply.
Notifications exceeding `Options::max_output_bytes` are also dropped.
Progress draining is bounded per dispatcher turn so a busy producer cannot
keep the dispatcher collecting notifications indefinitely. A successful enqueue
does not guarantee delivery before the request ends.

## Extension methods

List product methods in `SessionOptions::extension_methods` (requests) and
`extension_notifications`. Legacy requests and registered notifications reach
your handler after initialization. Modern custom RPCs use per-request metadata
and require no handshake. Return `Outcome::Method` for a request and
`Outcome::Done` for a notification.

Modern registration cannot enable protocol methods such as `ping`,
`resources/subscribe` or `subscriptions/listen`. Enable [Prompts](prompts.md)
through its catalog and [subscriptions](subscriptions.md) through their policy
rather than extension registration. Unavailable standard
methods return `-32601`. Handshake-based clients retain `ping`. The driver
validates modern protocol error codes and required data before publication;
see [independent-runtime encoding](session.md) for the same boundary.

Optional [Tasks](tasks.md) uses `Request::Task` and `Outcome::Task`; its methods
are protocol-owned and must not be registered as product methods. Returning a
handle releases the original RPC's worker and key. Your execution service owns
the job and its resource ordering after that point.

## When the host disconnects

When stdin closes:

- queued calls to tools not annotated `readOnlyHint: true` are answered with
  `-32800` and never run;
- a pending confirmation is withdrawn and its continuation never runs, and
  work that asks for confirmation afterwards is answered with `-32800`
  without a form;
- running mutations get `Options::disconnect_grace` (250 ms by default), then
  their contexts are cancelled;
- read-only calls, queued or running, finish within their deadlines.

`serve` returns after that work settles. Annotate tools accurately: the
read-only hint decides what may still run.
Resource reads follow the same read-only lifetime; product extension requests
follow mutation rules.

## Diagnostics

`Options::observer` receives every frame in both directions. Use it to record
bounded diagnostics; never write to stdout from it. It runs inline and must
return quickly.

For lifecycle timing without payload bytes, configure a bounded event channel:

```rust
use canonic_mcp_kit::stdio::{Options, observation_channel};

let (observations, records) = observation_channel(1024).expect("valid observation capacity");
let options = Options {
    observations: Some(observations.clone()),
    ..Options::default()
};
```

Drain `records` from your own diagnostic worker while `serve` runs. The library
starts no exporter and never waits for receiver capacity: full or closed
receivers drop records. `observations.dropped()` reports cumulative loss across
all producer clones; a nonzero count means the evidence is incomplete. Capacity
must be within 1–4,096 records. Drop the options and all producer clones when
your exporter needs to observe channel closure.

Records identify a connection, input, lane, worker and execution assignment.
Use `(connection, request)` to correlate work; protocol IDs can be reused while
an old cancelled worker still runs. Legacy continuations retain their input
identity and receive new assignments. Modern retries are independent inputs.
Protocol IDs are omitted by default. `observations.with_rpc_ids(true)` enables
bounded inbound string/integer IDs on that producer clone. Request IDs are
client-selected; confirmation and elicitation answers can echo server-issued
question IDs. Early records precede ID binding, so correlate them by internal
identity rather than protocol ID alone.

`ObservationKind::Frame::bytes` counts observed bytes without the trailing
newline. For rejected input this is a lower bound on the complete frame size:
the bounded reader stops at its limit rather than reading the rest of an
oversized frame.

`ObservationKind::Start`/`End` report inclusive intervals for decode/input
validation, callbacks, lane queues, workers, handlers, result validation,
encoding, host-input waits, output and cleanup. `Observation::parent` identifies
nested intervals. Decode includes routing and prepared schema work; encoding
can include result-schema validation. Worker intervals include completion
collection, while `OutputQueue` also measures completion delivery and prepared
frame queueing. Intervals can overlap; do not sum them into exclusive CPU time.
Reader time includes external input waits. Host-input waits include form
publication and are not measurements of human decision time alone.

`Settled` records the protocol decision; `Released` records the last live owner
leaving. A cancelled or expired worker can exit later, with its result recorded
as `Discarded`. Frame kinds distinguish ordinary replies, forms, progress,
subscription output and form cancellation. Subscription inputs remain live
through their final result or cancellation. A failed write/flush records
`PublicationFailed`; peer receipt is unknown. `ReaderDetached` means a blocked
reader can outlive an early driver return. Disabled lifecycle accounting creates
no diagnostic records, timestamps or history map. These records do not measure
catalog construction or executable startup before `serve`.

## Errors that end the connection

An oversized or unterminated frame, a request reusing an outstanding ID and
an exceeded `SessionOptions::max_messages` end the connection after an error
reply; `serve` returns `ServeError::Protocol`. See [errors](../reference/errors.md).
