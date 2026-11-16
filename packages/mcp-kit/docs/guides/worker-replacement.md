# Replace implementations without reconnecting the host

Enable `worker-process` when your server needs a stable MCP frontend with
replaceable implementation processes. The frontend retains its stdin/stdout,
HTTP listener, sessions, catalogs and scheduler. Compatible implementation
processes switch between completed workflows. Existing calls finish on their
original process; installing a file does not automatically initiate a switch.

```toml
[dependencies]
canonic-mcp-kit = { path = "../mcp-kit", features = ["worker-process"] }
canonic-process-kit = { path = "../process-kit" }
```

The API is available on macOS, Linux and Windows. Your native process adapter
owns executable authentication, private channel I/O, frontend-death detection
and exact process cleanup. The library supplies the MCP/controller composition;
it does not supply a universal process launcher or resource migration service.

## Prepare the frontend

Implement `worker::Implementation` for your authenticated native adapter. It
extends Process Kit's `worker::Worker`: lifecycle callbacks have absolute
deadlines and candidates remain inert until activation. Its additional methods
provide a complete `worker::Contract` and execute one typed `stdio::Request`
with the originating `stdio::Context`.

Build each contract using `Contract::new(&catalog, &options, resident, bytes)`.
It captures every tool and the complete session/scheduling configuration,
including schemas and immutable resource/Skills/Apps content. `resident` is a
bounded JSON object declaring your frontend identity, initialization/admission
policy, embedded documentation digest, authorization policy and durable-state
formats. Use the same declarations in the frontend and candidate. Rust
callbacks cannot be compared automatically; your adapter must authenticate the
candidate's declaration and ensure it represents the actual implementation.

Acquire a Process Kit domain `Lease`, then create `Frontend::new` with the
frontend identity, already active implementation, explicit `SupervisorOptions`,
aggregate retained-input byte capacity and resource-hold capacity. Supervisor
dispatch capacity must cover the sum of configured lane capacities. There is
one frontend attachment per controller. Multiple independent frontends require
isolated writer domains or your own authoritative coordinator.

Attach it through the ordinary driver options:

```rust
# #[cfg(all(feature = "worker-process", any(unix, windows)))]
fn attach(
    frontend: std::sync::Arc<canonic_mcp_kit::worker::Frontend>,
    mut options: canonic_mcp_kit::stdio::Options,
) -> canonic_mcp_kit::stdio::Options {
    options.worker = Some(frontend);
    options
}
```

Pass those options to `stdio::serve`, or to `http::serve` with `http-server`
enabled. The resident `Handler` still owns `connected`, `initialize`,
`admit`/`admit_context` and `invalid_arguments`; its `call` is bypassed for
implementation work. Bundled reads remain in the frontend. HTTP keeps its
modern-only protocol profile and original per-POST routing and authorization.

## Carry work over a private channel

`worker::wire` provides strict bounded encoding/decoding, assignment envelopes,
typed request restoration and terminal result reconstruction. Assignments have
frontend/worker incarnations and a non-reused sequence, independently of host
RPC IDs. Use `Call::check` before dispatch and `Reply::outcome` with the exact
originating assignment. The latter rejects foreign responses and rebuilds core
types; the scheduler then applies the original producer schema and revision
encoder. Never forward a worker's raw JSON-RPC bytes to host output.

Bound frame lengths before allocation with Process Kit framing, then call
`wire::decode` for duplicate-key and typed-envelope checks. Supply finite
aggregate channel/manifest budgets and absolute I/O deadlines. Explicitly flush
buffered private writers after complete frames. A partial exchange is uncertain;
do not reconnect and replay it. `Call::new` captures modern metadata and
token-free grant data; opaque grant extensions require your own codec and are
refused by this helper rather than discarded.

On the worker, prepare one `wire::Decoder` from its catalog/session options.
`Call::restore` checks the exact expected assignment and rebuilds the request
and handler `Context` through that decoder's owning validators. Pass the
decoder, an assignment control with the original
monotonic deadline and an optional bounded `ProgressSink`. Restoration does not
create a session, initialize a host or renew a timeout. It preserves request
metadata, client/retry inputs, task eligibility, token-free grants and the
catalog's prepared result schema and resource cache policy. The frontend still
encodes the host reply. Private wire version 2 carries this complete context;
older wire versions refuse before execution.

The adapter must account for transit and queue time before creating that control
and forward cancellation until actual execution ends. One conservative private
handshake is for the source to request readiness for the assignment, the receiver
to record a monotonic checkpoint before answering, then the source to capture
its remaining budget after observing that readiness. Idle time before requesting
readiness is outside this exchange. Adding the captured budget to the receiver checkpoint cannot
gain the readiness exchange's time. Do not start a fresh relative timeout after
decoding or waiting for an execution slot.

`Reply::terminal` borrows a supported terminal outcome and encodes its payload
under the supplied byte budget. Encode the complete reply under the channel
frame limit too. Unsupported legacy continuations remain owned by the caller
for its proxy registry; the terminal encoder cannot serialize them. Restored
remote contexts refuse `worker_hold`/`worker_publisher` because a worker cannot
allocate a frontend reservation locally. Add an explicit resource protocol
before allowing worker-backed resources to escape a call.

A `ProgressSink` must enqueue without blocking and report saturation. The
restored context gates it on the original control and progress-token presence;
dropping that context also disables reporters captured from it. The native
adapter still rejects stale queued progress after assignment settlement.

Your adapter forwards cancellation and progress using the original context.
Return `Ok(Outcome::Method(Err(error)))` for a domain error. Return `Err(error)`
for an exchange/protocol/process failure, using `worker::Failure` to preserve
execution certainty. `Settled(error)` requires proof execution has ended and
allows explicit compatible replacement. `Uncertain(error)` retains a process
hold until explicit shutdown proves cleanup; converting `RpcError` into
`Failure` selects this conservative variant. Neither implies rollback or allows
replay. Both disable future dispatch on that generation, including its queued
work. Panics also retain uncertain ownership. Successful results must prove
execution ended, including after host cancellation.
Later calls to a failed generation report that operator recovery is required;
they do not suggest retrying a temporary capacity refusal. Admission paused by
replacement or closure has a separate refusal, while byte/dispatch saturation
uses the ordinary server capacity error.

Legacy confirmation/elicitation outcomes use local proxy closures that resume
opaque tokens on their original worker. The adapter owns that bounded remote
continuation registry and its cancellation acknowledgements. Closures, pointers
and prepared validators never cross the wire. The provided terminal codec does
not encode those consumer interaction tokens. Advertise only the capabilities
your native adapter actually supports.

The frontend retains the source reservation through destruction of a proxy
closure as well as its execution. Proxy destruction must remain nonblocking;
if remote abandonment finishes asynchronously, keep an explicit resource hold
until the worker acknowledges cleanup.

## Preserve resources beyond an RPC

Take `Context::worker_hold()` before acquiring a worker-backed resource or
publishing a worker-owned job. Retain the returned `Hold` until actual cleanup
or explicit verified adoption. A task handle, returned result or cancelled RPC
does not prove that an executor, browser or device connection has stopped.
Modern continuation state likewise needs an explicit compatible state format
or a retained hold until settlement/expiry.

For a worker-backed subscription producer, use
`Context::worker_publisher(resident_change_sender.clone())`. The returned
`Publisher`, its clones and each queued change pin the original process.
Drop the producer only after it has stopped generating changes. This prevents
an update overtaking unpublished source changes; passing an unbound resident
`ChangeSender` into a worker adapter cannot establish this guarantee.

Frontend-only streams retain their IDs/filters/deadlines without pinning an
implementation. Worker-backed producers, pending legacy continuations and
other unsupported resources defer replacement instead of being cancelled.

## Request a replacement

On your update/control thread, call `Frontend::replace` with a monotonic
`worker::Request`, an authorized candidate launcher and your commit callback.
The launcher receives an absolute preparation deadline. Do not call replacement
from an inline `Handler` callback or block the dispatcher on its result.

The controller pauses new handler admissions with the existing busy refusal;
it does not buffer unassigned calls. Already admitted queued/running work and
continuations retain their original process. Ping, cancellation, interaction
answers and output continue through the existing dispatcher. Catalog publication
waits in the bounded change queue for the attempt to resolve. Source-backed
queued publication can therefore cause a safe deferral; retry selection after
those obligations settle using a fresh attempt sequence.

The complete candidate contract must match. An update changing schemas,
capabilities, inline policies or immutable bundles needs a frontend reconnect.
Host-visible identity describes the frontend; worker builds are diagnostic
values. Replacement sends no new initialization handshake.

## Handle deferral and uncertainty

Inspect `Frontend::status()` and `outcome(&request)` after errors. Before fencing,
a proved candidate cleanup reopens source admission. After fencing, failures
retain handles for reconciliation and never silently reactivate the source.
`Commit::Unknown` remains unknown even after successful process cleanup.
Request payloads and completed RPC history are not retained for diagnosis.

The driver latches closure, preserves its normal admitted read-only EOF rules,
joins RPC workers and attempts exact native cleanup. A cleanup failure returns
`ServeError::Worker`; keep the frontend handle and call `shutdown` again after
the outstanding resource or native failure is resolved. If another driver error
also occurred, inspect retained controller status for cleanup uncertainty.
`close()` latches immediately; `shutdown()` returns busy rather than waiting on
a competing replacement. Native callbacks must honor their deadlines.

This mode preserves transport continuity; it does not migrate active stacks,
replace the frontend binary, adopt arbitrary resources or discover installed
versions. Each native adapter needs acceptance on its target platform.
