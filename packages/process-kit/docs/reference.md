# API reference

| Module | Use |
| --- | --- |
| `core` | `Admission`, RAII `Permit` and `Hold`, `Snapshot`, `State`, `Event`, `ExitPolicy` |
| `endpoint` | `Root`, `Home`, `Lease`, borrowing `Endpoint`, peer checks and non-launching connect |
| `net` | Platform `Stream` and `Listener` re-exports for local socket I/O |
| `frame` | Four-byte big-endian length prefix, bytes, JSON read/write convenience |
| `protocol` | `Identity`, `Control`, `Request`, `Reply`, `Code`, independent `CONTROL_VERSION = 1` |
| `version` | Bounded semantic `Version` parsing and release precedence |
| `launch` | `Launcher`, current-executable `Spawn`, registered `Service` activation |
| `client` | Explicit connect-or-start, one-shot call, connection open, control, upgrade |
| `recovery` | Bounded `call`, replay `Options`, product reconciliation `Decision` |
| `owner` | `Connection` holds resource responsibility while detecting owner-channel EOF |
| `connection` | `Handoff`, product-owned `Connection`, transferable lifetime/capacity `Guard` |
| `server` | Synchronous `serve`, `Handler`, dispatch `Context`, bounded `Options` |

All public fallible functions document their errors in rustdoc. `Error` is a
typed `thiserror` enum. The package exposes no CLI and writes no logs or product
records. The product opens its log and gives the resulting file to `Spawn`.

## Local streams and endpoints

Use `net::Stream` and `net::Listener` when naming the socket types returned by
the library. The `net` module is available on Unix and Windows builds; these
are re-exports of the platform types, with their native methods:

| Platform | `net::Stream` | `net::Listener` |
| --- | --- | --- |
| Unix | `std::os::unix::net::UnixStream` | `std::os::unix::net::UnixListener` |
| Windows | `uds_windows::UnixStream` | `uds_windows::UnixListener` |

The Windows type re-exports do not enable manager connections: Windows root
constructors and peer checks return `Error::Unsupported`. The manager runtime
is supported on macOS and Linux.

For a custom driver, `Lease::bind(control)` returns an `Endpoint` borrowing the
lease. `Endpoint::listener() -> &net::Listener` borrows its bound listener.
`Endpoint::path() -> &Path` returns the socket pathname without filesystem I/O:
`manager.sock` for the ordinary lane or `control.sock` for the control lane,
under the selected root. Reading that path does not establish liveness or
transfer cleanup authority; retain the endpoint and lease guards.

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

Custom policies implement `exit(idle)` and `next_check(idle)`. The latter returns
the remaining time until another idle evaluation or `None` to wait for activity.
Zero requests immediate evaluation. The driver sleeps until that deadline,
socket activity or an admission notification; `Explicit` has no periodic idle
timer. Live work and holds suppress idle timers. Use `Admission::wake()` after
changing a policy input outside admission; this preserves the idle epoch.

Alternative drivers reserve controls with `Admission::control()` and call
`Permit::classify_control(role)` after validation, before acknowledgement or
effects. Classification atomically applies the role and idle-accounting rules.
They register their standard Rust `Waker` with
`Admission::set_waker` before observing state, call `tick` and
`Admission::next_check`, then await notification or the returned timer. Clear
queued notifications before observation, never after computing the wait.
Changes to permits, holds and shutdown wake outside the admission lock. Detach
the waker with `set_waker(None)` when the driver stops. No async dependency is
required.

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
their role from `Handler::control_role(&Control)`; its default is
`core::ControlRole::Work`. Choose `Inspection` for read-only actions: they do
not renew idle grace, cannot veto retirement, and `permit.hold()` returns
`Error::ReadOnly`. Choose `Cleanup` for operations that settle existing resources
during drain. Cleanup retains a work reservation through the reply and can
transfer slow cleanup into a hold. Ordinary work is refused during drain.
Cleanup is refused once existing work and holds settle, even if an earlier
observer is still awaiting classification. Products must validate exact cleanup
authority and prevent unrelated new work. All callbacks must return promptly;
product workers perform slow cleanup so controls remain responsive.
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

## Long-lived connections

Use `client::open(&root, &request, &options)` for a duplex stream or a persistent
client-presence channel. It never starts a manager. The handshake requires
`connection-handoff-v1` and returns `(Reply, net::Stream)` after consuming exactly
the admission and result frames. The stream is blocking with transport timeouts
cleared; set application deadlines or transfer it to your runtime. A product
error or oversized reply returns `Error::HandoffRefused` containing the reply.
Transport loss remains `OutcomeUnknown`; reconnecting does not authorize replay.

Implement `Handler::open` to return `connection::Handoff::new(reply, receiver)`.
The receiver gets a `connection::Connection` only after the final reply write
succeeds. It must enqueue or transfer that connection promptly; running the
session inline occupies the handshake worker. The library creates no session
threads. Prepared resources should use captured RAII guards so failed replies
clean them up without invoking the receiver. Never write product stream bytes
through `Context::stream` during the handshake.

Borrow `Connection::stream()` for I/O. Use `into_parts()` to obtain a
`net::Stream` and `connection::Guard` for an async adapter. Retain that guard
through all socket copies and resource cleanup, including after EOF. Dropping
it releases the lifetime hold and connection slot. The product owns subsequent
framing, cancellation and EOF semantics.

`server::Options::connection_capacity` defaults to 64 and counts prepared plus
transferred connections through cleanup. Zero disables handoff and its
capability. Exhaustion returns `Capacity` before `Handler::open`. Transferred
connections free ordinary lane capacity; status reports occupied connection
slots, including preparations, as `connections.transferred`. Their holds keep
retirement busy.
The product's drain callback must close or clean up these sessions. Coordinate
the receiver with drain state: a handoff admitted before drain may finish
after that callback, and its receiver must then clean up immediately.

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

On macOS and Linux, `owner::Connection::new(stream, hold)` attaches a resource
hold to a dedicated silent same-user connection. The product must arrange for
the owning process to retain the other endpoint, bound these channels and drive
`wait(timeout)` on its runtime. Do not share the read side with request framing. A timeout
returns false; EOF, including after owner death, returns true. Unexpected data
or a socket failure returns an error. The hold remains live after EOF: clean up
the resource before dropping the connection, and retain it if cleanup is
uncertain. EOF requires every copy of the peer endpoint to close.

Windows owner-channel construction and `wait` return `Unsupported`. A wait
refusal does not report owner death or release a retained resource hold.

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
| `connection_capacity` | 64 | Prepared plus product-owned connections through cleanup; zero disables handoff |
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

`Service` has `Launchd(String)`, `Systemd(String)` and `ScheduledTask(String)`
variants. Activating a variant on a different platform returns `Unsupported`.

Windows `Service::ScheduledTask` activates an existing task using
`SystemRoot/System32/schtasks.exe`. It requires an absolute `SystemRoot`;
missing, empty or relative values return `Options`. Task names beginning with
`-` or `/` also return `Options`.

`Absent`, `Starting`, `Draining`, `Capacity` and `Incompatible` are distinct.
`Starting` means readiness is unconfirmed or an authorized startup wait expired.
On macOS it also covers a refused leased endpoint: the OS uses the same error
for a full backlog and a stale/closed listener, so those cases cannot be safely
distinguished. Interrupted kernel connection polls retry on the same socket
within the original deadline.
Local frame rejection writes nothing. A complete refusal is definite
non-admission. After any potentially partial send, lost admission or result
produces `OutcomeUnknown(request_id)`; inspect the product's durable outcome.
Malformed JSON and selected-envelope decode failures receive a bounded
`Code::Invalid` refusal attempt before closure, with no product callback.
The refusal preserves a valid bounded request ID when available and uses an
empty ID otherwise. A lost refusal remains ambiguous to the caller.
Never infer non-admission just because an acknowledgement was not received.
One-shot calls never replay. Explicit recovery requires the product decision
described above. Products allocate stable IDs and deduplicate durable mutations
by ID and content.

`Lease::acquire` returns `AlreadyRunning` when its bounded wait expires. It does
not read a PID or delete lock anchors. An endpoint borrows its lease. Call
`Endpoint::unlink` to observe cleanup errors; drop also attempts identity-checked
cleanup. Preserve errors for diagnostics without using them as cleanup authority.
