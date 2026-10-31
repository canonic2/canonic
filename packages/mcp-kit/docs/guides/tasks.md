# Return a task for long-running work

Tasks let a tool return a tracking ID while work continues or waits for user
input. The host polls that ID and retrieves the final result later, including
after reconnecting. Ordinary tool results remain available. This is optional
protocol support: it does not require an async Rust runtime.

MCP Kit validates and encodes the messages. Your server owns the job, durable
records, access checks, resource leases, recovery and cleanup. Enabling Tasks
does not start an executor or create a database.

## Try the complete example

The [Acme example](../../examples/tasks.rs) serves a report workflow backed by
[consumer-owned files](../../examples/tasks/store.rs). It requires a Unix host
for its durable directory-flush implementation. The protocol API itself has
no filesystem or Unix dependency.

From `packages/mcp-kit`, run:

```sh
cargo run --locked --example tasks -- /tmp/acme-task-store
```

Choose a private directory used only by this example. Opening it sets Unix
permissions to `0700`, including when the directory already exists.
It stores a bounded set
of report records, holds a kernel lock to admit one owner, and reopens those
records on restart. The example treats IDs as bearer credentials inside that
local directory's access scope. A production server must choose and enforce
its own access policy; client names are not authenticated identities.

Send this single-line request on stdin:

```json
{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"acme_report","arguments":{"deferred":true},"_meta":{"io.modelcontextprotocol/protocolVersion":"2026-07-28","io.modelcontextprotocol/clientCapabilities":{"extensions":{"io.modelcontextprotocol/tasks":{}},"elicitation":{"form":{}}}}}}
```

The reply has `resultType: "task"`, a random `taskId` and status
`input_required`. The record is committed before the handle is returned.
Replace `TASK_ID` below with that returned ID:

```json
{"jsonrpc":"2.0","id":2,"method":"tasks/get","params":{"taskId":"TASK_ID","_meta":{"io.modelcontextprotocol/protocolVersion":"2026-07-28","io.modelcontextprotocol/clientCapabilities":{"extensions":{"io.modelcontextprotocol/tasks":{}},"elicitation":{"form":{}}}}}}
```

The complete snapshot contains `inputRequests.report-approval`. The host
presents the yes/no question to its user, then submits that user's choice:

```json
{"jsonrpc":"2.0","id":3,"method":"tasks/update","params":{"taskId":"TASK_ID","inputResponses":{"report-approval":{"action":"accept"}},"_meta":{"io.modelcontextprotocol/protocolVersion":"2026-07-28","io.modelcontextprotocol/clientCapabilities":{"extensions":{"io.modelcontextprotocol/tasks":{}},"elicitation":{"form":{}}}}}}
```

The update is acknowledged with `resultType: "complete"`. Poll again: status
is `completed` and `result` contains the original tool payload. A decline also
completes this workflow, with a tool error explaining the declined report.
The tiny executor runs after durable input consumption; longer work belongs
on your own execution service.

Close stdin, rerun the same command with the same directory and poll the ID
again. Unexpired questions and terminal results remain available. A crash
after input consumption but before result commitment is recovered as `failed`
with an uncertainty message; the example never replays report generation.
Records expire after ten minutes from creation. New creates reclaim expired
records, and reopening prunes them. Reads do not extend retention.

For an ordinary result, call `acme_report` with `deferred: false` or omit the
argument. That preview also works with existing legacy clients. A modern
deferred call without Tasks support gets a missing-capability error before
creating a record.

## Add Tasks to your server

Enable `SessionOptions::tasks` together with `SessionOptions::modern`. Supply
an explicit `task::TaskOptions` value: retained-record and byte quotas,
execution capacity, input-round limit, execution timeout and input timeout.
All limits must be positive and finite; there are no library policy defaults.
Your storage/execution service enforces them. `TaskOptions::admit` checks a
proposed reservation, but does not reserve storage or synchronize writers.

Publish the extension only after integrating all three management methods.
Do not register them as product extension methods. MCP Kit reserves and routes
them through `Inbound::Task` and `stdio::Request::Task`.

The server decides per call whether to return a task. Check
`RequestContext::task_eligible()` before effects. It requires an eligible
modern tool call, server opt-in and that call's Tasks declaration. Support on
another request does not carry forward. `require_tasks()` provides the typed
missing-capability refusal; a server needing ordinary fallback must choose it
before launching work.

After validating authority and reserving capacity, commit a discoverable task
record and its recoverable execution intent. Construct `task::Metadata` with
an opaque `TaskId`, consumer-supplied `Timestamp` and finite TTL. Then return:

```rust,ignore
Outcome::Task(Ok(TaskResult::Created(snapshot)))
```

This fragment assumes a validated, durably committed `task::Snapshot`. The
complete example contains the imports, construction and persistence code.
Metadata and snapshot constructors check bounds. In the persistence
representation, rich input includes the storage-only
`inputKind: "rich"` marker, so even an empty rich form retains its answer
semantics after reopening. Untagged basic question records remain readable.
Use `RequestContext::encode_task` for wire replies; it omits this marker.
`Snapshot::restore` validates stored values without authenticating the store
or recovering execution.
`Timestamp::from_unix_millis` converts a supplied clock reading without reading
the clock itself. TTL is measured from creation, not completion or last access.

## Handle management calls

| Request | Consumer action | Successful outcome |
| --- | --- | --- |
| `TaskRequest::Get` | Authorize the ID, check expiry and return a consistent snapshot promptly | `TaskResult::Snapshot` |
| `TaskRequest::Update` | Validate known pending input and durably consume it with continuation intent | `TaskResult::Updated` |
| `TaskRequest::Cancel` | Durably record cancellation intent or acknowledge an already terminal task | `TaskResult::Cancelled` |

Wrap each outcome in `Outcome::Task(Ok(...))`. Return a `RpcError` with
`Outcome::Task(Err(...))` for management failures. `task::not_found()` deliberately
does not distinguish an unknown, expired or inaccessible ID. Storage outages
are internal errors, never not-found or empty success.

Each call has its own RPC ID, context and admission deadline. Put management
operations on lanes that long jobs cannot occupy. Get is read-only for EOF
handling; update and cancel are mutations. Creation retains its tool's
ordinary admission and read-only classification. Returning a handle releases
the original RPC's lane slot and key. Preserve job-level ordering and resource
ownership in your execution service.

`tasks/cancel` acknowledges intent, not completed cancellation or rollback.
Polling can still show a completed result when work wins the race. In
contrast, `notifications/cancelled` targets an outstanding RPC: cancelling a
poll or the original, already answered call does not cancel the task.

## Preserve state and input integrity

`task::State` distinguishes working, input-required, completed, failed and
cancelled snapshots. A tool result with `is_error: true` is `Completed`; use
`Failed` for a JSON-RPC/executor failure. `Snapshot::validate_successor`
checks immutable identity, clock ordering, retention and terminal immutability.
Commit with a compare-and-set or equivalent owner check: pure validation
cannot prevent two concurrent writers from settling different outcomes.

The basic input kind is the existing yes/no form. Create a `Question`
using the originating request context. Persist its meaning, authority scope,
expiry and lifetime-unique key. `InputHistory` helps reject key reuse and
enforces your input-round limit; persist it with the question and count its
bytes in your quota. Do not remove consumed or superseded keys prematurely.

Use `Snapshot::input_decision` against a validated update. It checks expiry,
task identity, pending input and current form capability. Unknown or consumed
keys are ignored; malformed known answers fail without consuming input.
Commit consumption and continuation intent before acknowledging. Revalidate
authority and action identity before effects; a host answer alone grants none.

For several questions, fields or URL consent, use
`State::RichInputRequired(elicitation::Requests)` and `Snapshot::input_answers`.
It validates the full update before returning known answers, including partial
valid sets. Persist answers and continuation intent before acknowledging and
track every `pending_keys()` value. The [richer input guide](elicitation.md)
explains construction, capability checks and external completion.

Complete ordinary multi-round-trip confirmation before creating a task.
After creation, input flows through get/update, without `requestState` or a
retry of the original tool. Sampling is outside the supported task-input profile.

## Recovery and retention

Published tasks outlive the connection, not automatically the server process.
Persist enough state to reopen them or delegate execution to an owner that
survives process exit. On recovery, reconcile unfinished jobs or settle them
as failed, preserving uncertainty about effects. Do not blindly replay work.

A record can be committed even when its handle reply is cancelled, oversized
or lost. Keep that record for reconciliation and cleanup. A client retry is
not automatically deduplicated; consumer idempotency keys require their own
operation contract. The library provides no task enumeration or exactly-once
execution guarantee.

Reserve capacity for terminal results and bounded failure settlement before
starting effects. Refuse creation at capacity rather than evicting unexpired
records. At expiry, reject input and access, and retain internal ownership
information if cleanup still needs it. Deleting a handle does not stop a job.

## Limits and runtime integration

| Value | Library limit |
| --- | --- |
| Task ID or input key | 1–128 UTF-8 bytes; no control characters |
| Status message | 4096 UTF-8 bytes |
| UTC RFC 3339 timestamp | 64 UTF-8 bytes |
| TTL and optional polling interval | Positive safe JSON integer milliseconds; null TTL is outside this profile |
| Pending input | One basic yes/no question or up to eight rich questions; map at most 64 KiB and 32 containers deep |
| Input response map | 16 KiB and 32 containers deep |
| Detailed snapshot | 1 MiB encoded JSON and 64 containers deep |

Frame, metadata and output budgets also apply. Results are refused rather
than truncated. Large reports should remain consumer-owned artifacts with a
compact tool result. No result streaming, subscriptions or task notifications
are provided. Progress belongs in task snapshots, not on the completed RPC.

An independent runtime drives `Inbound::Task` just like other admitted work.
Capture `Session::request_context`, call your service, and encode typed
outcomes with `RequestContext::encode_task`. Send only after `Session::finish`
returns true. The encoder enforces method/record eligibility, current client
capabilities, server identity, byte limits and its interruption callback.
Management replies require this typed encoder; `encode_success` cannot bypass
the task outcome checks. The core starts no threads and performs no I/O.
Dropping a future cannot undo a persisted task or input response.

See [session integration](session.md) and [stdio scheduling](stdio.md) for the
surrounding request lifetime, and [errors](../reference/errors.md) for refusal
and recovery paths.
