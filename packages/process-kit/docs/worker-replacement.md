# Replace workers behind a stable frontend

Use `worker::Supervisor` when your process must keep its host connection open
while replacing an implementation worker. The frontend retains stdin/stdout or
network streams. The supervisor keeps one dispatch target, pauses new dispatch,
waits for existing calls and holds, and controls an irreversible cutover.

This is an adapter API, not an MCP proxy or an installed-version watcher.
Your protocol adapter retains framing, session negotiation, response routing,
cancellation and bounded queues. Your process adapter launches authenticated
workers and arranges independent cleanup when the frontend dies. Compatible
worker replacement does not update the frontend itself.

## Establish ownership

Acquire a private domain `Lease` and pass it to `Supervisor::new` with a fresh
frontend `Identity`, an already active `Worker` and explicit `Options`.
The controller retains the lease through replacement and shutdown. Outstanding
`Dispatch` reservations and resource holds also retain their original worker
and exclusion if the controller is dropped.
Products with shared state still need their own worker writer locks and durable
authority records; the frontend lease is not that writer authority.

```rust
use canonic_process_kit::{Result, endpoint::{Lease, Root}, protocol::Identity,
    worker::{Options, Supervisor, Worker}};
use std::time::Duration;

fn start<W: Worker>(root: &Root, start_active: impl FnOnce() -> Result<W>)
    -> Result<Supervisor<W>>
{
    let lease = Lease::acquire(root, Duration::ZERO)?;
    let frontend = Identity::new("1.0.0".into(), 7, vec![])?;
    let active = start_active()?; // Authenticate and initialize under exclusion.
    Supervisor::new(lease, frontend, active, Options {
        preparation: Duration::from_secs(5),
        activation: Duration::from_secs(5),
        cleanup: Duration::from_secs(5),
        capacity: 64,
        history: 32,
        history_bytes: 1024 * 1024,
    })
}
```

These are example limits. Durations must be positive and fit the monotonic
clock; `capacity` and `history` are positive and at most 65,536. `history` counts
the current attempt too. `history_bytes` is 64 KiB through 64 MiB, reserving one
64-KiB slot for current progress. Protocol queue bytes are a separate adapter limit.

Implement `Worker` around an exact owned process and its private control channel:

| Callback | Required proof |
| --- | --- |
| `identity` | Immutable identity of the authenticated process; a build string or same-user socket alone does not authorize the executable |
| `prepare` | Idempotent read-only proof that the candidate is inert and compatible, checked before settling calls and again against fenced state |
| `fence` | Predecessor writers and callbacks cannot authorize further effects; exclusion remains held for commit |
| `retire` | Transferred cleanup is disarmed, worker locks released and the predecessor reaped; retry is safe |
| `activate` | Successor acquired product authority and restored obligations; it can now accept dispatch |
| `stop` | Exact owned process and resources have settled and been reaped; retry is safe even after retirement |

Callbacks run synchronously outside supervisor/admission locks. They must honor
the supplied absolute deadline. A callback error must retain the handle's cleanup
responsibility. A failed launch must leave no untracked process. If launch returns
a candidate late, the supervisor still retains and cleans it up.
Panics are caught. Before fencing, successful candidate cleanup resumes the
predecessor; after fencing, failures leave replacement blocked for reconciliation.

Arrange worker owner-loss detection before exposing the frontend. A private
owner channel and `owner::Connection` are available for socket-based adapters;
hostile-user validation and inherited-handle isolation still apply. An arbitrary
child does not acquire parent-death cleanup just by implementing this trait.

## Retain dispatch through the response

The worker returned by a reservation remains its original target during
preparation. Use that target for the call and its cancellation, and retain the
reservation until the result is delivered or responsibility has moved to a
bounded frontend-owned queue. The supervisor never buffers or replays requests.

```rust
use canonic_process_kit::{Result, worker::{Supervisor, Worker}};

fn deliver<W: Worker, T>(
    supervisor: &Supervisor<W>,
    call: impl FnOnce(&W) -> Result<T>,
    write_reply: impl FnOnce(T) -> Result<()>,
) -> Result<()> {
    let dispatch = supervisor.enter()?;
    let result = call(dispatch.worker())?;
    write_reply(result)?;
    drop(dispatch);
    Ok(())
}
```

`enter` refuses with `Draining` while paused/closed and `Capacity` at the
configured in-flight cap. Your adapter chooses bounded backpressure or a
protocol-specific non-admission response. Status and the existing reservation's
worker remain accessible during preparation.

Use `Dispatch::hold()` for resources or uncertain outcomes that outlive the call.
It returns a `worker::Hold<W>`; `Hold::split()` pins another obligation with the
same worker and domain lease. Ownership remains retained until the last
reservation or hold drops, even after the supervisor is dropped.
Initial between-call replacement waits for these holds; it cannot adopt live
sessions or Rust continuations. Restore holds for any initial resources before
advertising frontend readiness. A long call/resource causes bounded deferral,
not forced cancellation.

## Request an upgrade

The adapter authorizes the installation and assigns increasing nonzero sequence
numbers within this frontend incarnation. `Selection::key` is a bounded ID for
the exact installation inputs, such as a digest; it is not an executable path
or a substitute for image validation. Map external stable request IDs to these
requests in the integrating product. Do not assign a fresh sequence to retry
an uncertain attempt.

```rust
use canonic_process_kit::{Result, worker::{Outcome, Request, Selection, Supervisor, Worker}};

fn upgrade<W: Worker>(supervisor: &Supervisor<W>, sequence: u64, launch: impl FnOnce(std::time::Instant) -> Result<W>)
    -> Result<Outcome>
{
    let request = Request {
        frontend: supervisor.status()?.frontend.manager_id,
        sequence, // Adapter's monotonic sequence; retain it for reconciliation.
        selection: Selection {
            key: "acme-image-digest".into(),
            build: "2.0.0".into(),
            protocol: 7,
            requires: vec!["acme_v1".into()],
        },
    };
    supervisor.replace(&request, launch, |_transition| {
        // Publish product ownership under its writer locks here. A stateless
        // worker with no surviving responsibilities needs no durable record.
        Ok(())
    })
}
```

The selected release must be semantically newer than the active worker. Actual
candidate identity/build/protocol/capabilities are checked before fencing.
`Worker::prepare` supplies the product's deeper compatibility proof. The old
process retires before candidate activation, so separate worker writer locks
can transfer without concurrent effects. The host-facing transport stays owned
by the frontend throughout.

The current exact request can be observed while replacement is running.
Identical retained requests return their outcome without another callback;
changed inputs or foreign/evicted sequences return `HandoverState`.
History is indexed by sequence and bounded by count and encoded bytes. Resolved
outcomes are evicted oldest-first, while unresolved handles/current attempts
remain retained. Status polling neither launches nor renews any phase budget.

## Failures and host shutdown

Before fencing, failed preparation cleans the candidate and reopens predecessor
dispatch only after cleanup is confirmed. Failed cleanup retains the handle and
blocks replacement. After fencing, callback failures keep dispatch closed in
`Reconcile`; neither version nor transport recovery authorizes replay.

`Outcome::fenced` and `Outcome::commit` retain whether authority was fenced and
whether commit was never started, uncertain or confirmed. `Stopped` proves
worker cleanup, not rollback of a possibly committed product operation.
Read the product's durable outcome before deciding whether it may be retried.
The supervisor's initial recovery path is explicit shutdown; it does not retry
commit/activation callbacks or silently select a different successor.

On host EOF, call `close` to latch cancellation immediately, then `shutdown`
to run the adapter's cleanup policy. If replacement owns a callback, shutdown
returns `HandoverBusy`; wait for that operation to finish and retry shutdown until
`Stopped` is confirmed. Replacement observes the latch at its checkpoints and
cleans up a late candidate; it can also finish immediately before shutdown wins
the race. A cleanup failure retains its
handle for a later `shutdown`. Cleanup attempts both retained workers within the
shared cleanup deadline, even if one stop fails. Successful handles are removed;
failed handles remain in `Reconcile`, and the first stop error is returned.
If replacement and its cleanup both fail, `Error::WorkerCleanup` retains the
original `cause` and the `cleanup` error independently.
Nothing can reopen dispatch after host closure.
Independent native/owner-channel supervision handles abrupt frontend death.

## Platform scope

The controller uses portable Rust and existing platform leases. The native
Acme fixture demonstrates separate frontend/worker processes, a retained host
stream, owner-pipe EOF and reaping on macOS. Linux/Windows execution, Windows job
containment, protocol-specific MCP legacy/modern behavior and product resource
adoption require separate integration acceptance. Generic fixtures do not prove
that an existing MCP server transparently upgrades.
