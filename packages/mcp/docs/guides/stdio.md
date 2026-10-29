# Serve over stdio

`stdio::serve` reads frames on a reader thread, handles the protocol on the
calling thread and runs your requests on worker threads. This guide covers
the choices you make.

## Lanes

A lane is a group of workers with its own queue and capacity. The default is
one lane with one worker and room for eight requests, so requests run one at a
time. Give slow reads their own lane so they never wait behind mutations:

```rust,ignore
use canonic_mcp::stdio::{Admission, Lane, Options, Request};

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

## Extension methods

List product methods in `SessionOptions::extension_methods` (requests) and
`extension_notifications`. They reach your handler as `Request::Method` and
`Request::Notification` after initialization. Return `Outcome::Method` for a
request and `Outcome::Done` for a notification.

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
bounded diagnostics; never write to stdout from it.

## Errors that end the connection

An oversized or unterminated frame, a request reusing an outstanding ID and
an exceeded `SessionOptions::max_messages` end the connection after an error
reply; `serve` returns `ServeError::Protocol`. See [errors](../reference/errors.md).
