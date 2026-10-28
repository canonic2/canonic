# API reference

| Module | Use |
| --- | --- |
| `core` | `Admission`, RAII `Permit` and `Hold`, `Snapshot`, `State`, `Event`, `ExitPolicy` |
| `endpoint` | Unix `Root`, `Home`, `Lease`, borrowing `Endpoint`, peer checks and non-launching connect |
| `frame` | Four-byte big-endian length prefix, bytes, JSON read/write convenience |
| `protocol` | `Identity`, `Control`, `Request`, `Reply`, `Code`, independent `CONTROL_VERSION = 1` |
| `version` | Bounded semantic `Version` parsing and release precedence |
| `launch` | `Launcher`, current-executable `Spawn`, registered `Service` activation |
| `client` | Explicit connect-or-start, one-shot call, control, upgrade |
| `recovery` | Bounded `call`, replay `Options`, product reconciliation `Decision` |
| `owner` | `Connection` holds resource responsibility while detecting owner-channel EOF |
| `server` | Synchronous `serve`, `Handler`, dispatch `Context`, bounded `Options` |

All public fallible functions document their errors in rustdoc. `Error` is a
typed `thiserror` enum. The package exposes no CLI and writes no logs or product
records. The product opens its log and gives the resulting file to `Spawn`.

## Admission and lifetime

Create `Admission`, acquire initial `Hold`s, initialize product state and bind
endpoints before `ready()`. The synchronous driver performs binding and readiness.
Reserve `enter(false)` before preflight, queueing or effects; preserve the permit
through final reply publication. Once decoded, `Permit::observe()` converts a
read-only request to an observer. Observers finish during retirement but do not
renew the idle deadline. An observer cannot create a new hold after closure.

`retire_if_empty()` closes admission and rechecks counts under one short lock.
Busy work reopens admission and emits `DrainRefused`; observers need not be zero
to commit retirement. `drain()` permanently closes new admission. The product's
drain callback starts its cleanup; held obligations keep the process alive until
it settles. `tick(now, policy)` returns true only when all holds and responses
settle. `now` is monotonic elapsed `Duration`, supplied by the runtime.

Read-only controls remain available during drain while work or product holds
remain. Once both settle, new controls are refused with `Draining`; already
admitted replies finish before exit. Continuing control arrivals cannot renew
that final response set or prevent lease release.

`IdleGrace(duration)`, `WhenEmpty` and `Explicit` are example policies. Implement
`ExitPolicy` for other behavior. Its callback runs without the admission lock;
the core rechecks the observed idle epoch before closing. Product state must be
represented by holds, rather than a separately sampled emptiness boolean.

The latest 64 payload-free transitions are retained. `events()` consumes them;
overflow drops oldest diagnostics, never obligations. Products decide logging
and persistence. `Snapshot` contains no credentials or request contents.

## Version handoff

Commands allowed to start or replace a manager can use
`client::connect_versioned(&root, &launcher, env!("CARGO_PKG_VERSION"), &options)`.
Detection runs when that function connects. Ordinary `connect` remains an
inspection/startup primitive and does not request version retirement.

The running manager's `Identity::build` and the supplied client version must be
complete semantic versions. Comparison uses numeric release and prerelease
precedence: `1.10.0` is newer than `1.9.0`, and `1.0.0` is newer than
`1.0.0-rc.1`. `+build` metadata does not establish upgrade order. Invalid or
unorderable labels return `InvalidVersion` without retirement. Equal or older
clients reuse the running manager and never request a downgrade.

An empty older manager closes admission, completes its replies, removes its
endpoints and releases its lease. The launcher can then start a successor.
If a busy manager advertises `retire-if-older-v1`, it records
`retirement_pending: true`, keeps serving, and retires atomically once work and
holds settle. This bypasses its normal empty grace, never a live hold. Products
must still check the returned identity's protocol and required capabilities.
Stored resources that retain holds continue to pin the old manager until the
product safely releases them; an explicit drain uses normal product cleanup.

The server independently checks the proposed version and exact incarnation
before scheduling retirement. Its version capability is advertised only for a
semantic build label and counts toward the capability limit. A pre-feature
manager uses the existing `retire_if_empty` fallback; a busy one retains its
normal exit policy. A stale launcher that starts another older manager causes
`Incompatible`, rather than another replacement cycle. The same readiness
deadline covers the entire handoff. Custom launcher duration remains product-owned.
Only identity probes and idempotent, incarnation-checked retirement controls are
retried during a handoff. Product requests are never sent or replayed by it.
If a replacement still reports an older release, update the launch registration
or launcher to execute the installed current binary before retrying.

## Controls and extensions

Control decoding is independent of the ordinary protocol and build. Both
endpoints accept control envelopes. The dedicated control endpoint refuses
ordinary envelopes. Built-ins are `identity`, `status`, `retire_if_empty`,
`retire_if_older` and `drain`. Retirement and drain require the exact
`manager_id` observed in identity.

`Handler::extensions()` advertises accepted control actions; `control()` handles
them. Declare each required capability in `Identity::capabilities` and in the
request's `requires` list. Unknown actions, wrong versions, missing capabilities
and stale target incarnations are refused before admission. Extensions receive
a mutation reservation and can transfer work into a hold. They must return
promptly; product workers perform slow cleanup so controls remain responsive.
`blockers()` returns a bounded secret-free summary. Use paginated extension
actions for large inventories; freshness and exact authority checks belong to
the product. Identity and status metadata must never contain secrets.

`client::upgrade` asks an observed manager to retire atomically. `Retired` permits
waiting for endpoint closure and starting a successor; `Busy` includes identity,
compatibility and product blockers. It never kills or implicitly drains busy work.
If replacement races that probe, it returns `Error::StaleManager`. Inspect again
and reconsider the upgrade before targeting the current incarnation; the helper
does not automatically retire the replacement. Low-level `client::control`
preserves the raw `Code::StaleManager` refusal reply.
For an automatic release comparison and complete replacement wait, use
`connect_versioned` rather than this immediate manual retirement primitive.

## Request and owner recovery

`client::connect` with a launcher waits through a draining manager within the
readiness budget and starts a successor after lease release. Without a launcher,
inspection reports `Draining` immediately. A timeout does not kill live work.
With a launcher, dropped identity replies and per-frame identity timeouts retry
within the overall readiness budget. Activation waits for the old lease holder
to release exclusion, even when its endpoints have already closed.
Each readiness wait invokes the launcher at most once. A child that has not
locked its lease yet is still starting; the client waits instead of spawning
duplicates. If readiness expires, a caller can initiate a new connection attempt.

`recovery::call(root, launcher, request, transport, recovery_options, reconcile)`
connects, verifies protocol/capabilities and sends a request. Set positive
`recovery::Options::attempts` and `timeout` to bound sends and elapsed I/O time.
Definite non-admission failures can reconnect. An ambiguous send reconnects and
calls `reconcile` with the unchanged request and ready manager identity before
any replay. Return `Decision::Recovered(reply)` for a final durable result,
`Replay` only when durable deduplication or proven non-execution makes it safe,
or `Unknown` to preserve `OutcomeUnknown`. The last send can still be reconciled
without permitting an extra send. Reconciliation and launch callbacks own their
execution deadlines. The library never infers mutation replay safety from a
dead process, a missing acknowledgement or an empty journal lookup alone.

`owner::Connection::new(stream, hold)` attaches a resource hold to a dedicated
silent same-user connection. The product must arrange for the owning process
to retain the other endpoint, bound these channels and drive `wait(timeout)`
on its runtime. Do not share the read side with request framing. A timeout
returns false; EOF, including after owner death, returns true. Unexpected data
or a socket failure returns an error. The hold remains live after EOF: clean up
the resource before dropping the connection, and retain it if cleanup is
uncertain. EOF requires every copy of the peer endpoint to close.

## Explicit force-stop

`server::Options::allow_force_stop` defaults to false. Enable it only when the
product can safely recover from a manager crash before successor readiness.
When enabled, identity advertises `force-stop-v1`. Send control-v1 action
`force_stop` with the observed exact `manager_id`, null arguments and required
capability `force-stop-v1`. Disabled support and stale targets refuse without
termination.

After admission the server attempts the bounded final result `{"stopping":true}`
then exits the **entire process** with code 75. It skips destructors, worker
joins, hold settlement and product cleanup even if the result is lost. The
kernel releases the lease; a successor acquires it before replacing stale
owned sockets and performing product crash recovery. It does not stop external
workers. Drain, deadlines, version replacement and request recovery never
force-stop automatically. If controls cannot respond, this library does not
signal a PID or bypass the live lease.

## Bounds

| Setting | Default | Meaning |
| --- | --- | --- |
| `server::Options::workers` | 8 | Ordinary workers |
| `capacity` | 24 | Active plus queued ordinary connections |
| `control_workers` | 1 | Reserved control workers |
| `control_capacity` | 9 | Active plus queued control connections |
| `frame_limit` | 1 MiB | Payload size, excluding four-byte prefix |
| `io_timeout` | 3 seconds | Total queued-frame read deadline; bounded response writes |
| `client::Options::readiness` | 15 seconds | Overall probe budget, shared by connection, writes, admission and identity replies |
| `io` | 3 seconds | Per-frame read/write deadline, including prefix and body; readiness probes use the earlier overall deadline |

Server capacities must be at least their positive worker counts and at most
65,536. Frame limits are 1,024 through `u32::MAX`. Timeouts must be nonzero.
All limits refuse explicitly. There is no truncation. Products enforce their own
handler and operation deadlines and separately bound retained owner channels.
Refusals on the accept thread use an absolute write budget of at most 20 ms,
capped by a shorter `io_timeout`, so backpressure cannot consume a worker's full
reply budget on that thread. Worker replies retain their configured I/O budget.

## Launch and errors

`Spawn` clears the environment, installs only explicit pairs, sets null stdin,
uses the product's log and directory, and creates a new session/process group.
The client reaps it on a dedicated thread while alive; `start_reaped()` returns
the join handle when the caller needs its exit result. An OS registration owns
the manager when `Service` activates it; registration/removal stay with the
product. Linux activation may need a custom `Launcher` that explicitly supplies
the user's session-bus environment; none is inherited automatically.

`Absent`, `Starting`, `Draining`, `Capacity` and `Incompatible` are distinct.
Local frame rejection writes nothing. A complete refusal is definite
non-admission. After any potentially partial send, lost admission or result
produces `OutcomeUnknown(request_id)`; inspect the product's durable outcome.
Never infer non-admission just because an acknowledgement was not received.
One-shot calls never replay. Explicit recovery requires the product decision
described above. Products allocate stable IDs and deduplicate durable mutations
by ID and content.

`Lease::acquire` returns `AlreadyRunning` when its bounded wait expires. It does
not read a PID or delete lock anchors. An endpoint borrows its lease. Call
`Endpoint::unlink` to observe cleanup errors; drop also attempts identity-checked
cleanup. Preserve errors for diagnostics without using them as cleanup authority.
