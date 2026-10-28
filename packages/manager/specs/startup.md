# Single instance and startup

## Single-instance lease

One kernel-owned lock on the persistent manager.lock file excludes all other
owners of a root. Never unlink the anchor or infer release from a PID or stale
socket. `Lease::acquire` uses a caller-chosen bounded wait and capped
exponential backoff ([limits](limits.md#time)), then typed `AlreadyRunning`.
A killed process releases its lock through the kernel; socket presence alone
is not liveness.

## Initialization

Startup acquires the lease before listening and before product replay. Products
initialize state under the lease, create initial holds and then serve. Racing
helpers converge on the lease holder. A loser must not replay product state
or bind endpoints as if it were the owner.

The same ordering applies to an [update successor](updates.md#ownership-handoff).
The product loads and validates durable state under its acquired lease and
establishes initial holds before readiness. A retirement acknowledgement alone
does not establish that the successor may initialize or that it is ready.

## Inspection and readiness

Inspection can detect a live lease with no endpoint as `Starting`, distinctly
from `Absent`. Readiness requires an
identity reply, with a deadline and backoff. Ordinary payloads are never sent
as readiness probes and never replayed by the library.

| Observation | Meaning |
| --- | --- |
| `Absent` | No reachable endpoint and no live lease holder; inspection does not start one. |
| `Starting` | A lease holder has not supplied a ready endpoint; an explicit startup wait may expire in this state. |
| Identity reply | Readiness is established for the returned incarnation, subject to its lifecycle state. |
| `Draining` | The manager has closed mutation admission; readiness must not report it as serving. |
| `Capacity` | A bounded lane refuses admission, or the kernel reports congestion/a connection-wait timeout; this is not proof of absence. |

Kernel backlog detection is platform-dependent. On macOS `ECONNREFUSED` can
mean either a full Unix-listener queue or a stale/closed listener. The lease
and pathname cannot distinguish those cases. A refused leased endpoint is
therefore conservatively `Starting`, meaning readiness is unconfirmed; it
does not prove the process is initializing. With no lease holder, the same
refused endpoint is `Absent`. Do not infer authority to replace an endpoint
from a refusal alone. Explicit server-lane refusals and identifiable kernel
congestion still report `Capacity`.

Only an explicit connect-or-start call with a `Launcher` may activate a process.
It waits through `Draining` until a successor can become ready; inspection
without a launcher reports `Draining` immediately. A deadline never forces
the exiting holder to release a live obligation.
With a launcher, a dropped identity exchange or its per-frame I/O timeout is
also temporary while readiness time remains. Read-only identity probes can be
retried without replaying application work. Activation waits until the existing
lease holder releases exclusion, including when its endpoints have already
closed and inspection reports `Starting`. Invalid identities, incompatible
controls and endpoint/peer validation failures remain errors.
Each readiness wait activates its launcher at most once. An unheld lease after
activation can mean the child has not acquired it yet; it does not authorize
another activation. If that activation never becomes ready, the wait expires
with `Starting`; the caller can initiate a separate connection attempt.
Startup polling backs off and stops at its deadline. A probe's connection and
frame operations must fit the remaining readiness budget. Custom launchers own
the duration and failure semantics of their activation callback; the library
does not cancel arbitrary product code.

Connection timeouts are capped by the remaining readiness time. Request writes,
admission reads and identity reads use the earlier of their frame deadline and
the same absolute readiness deadline; no phase renews the overall budget.
Expiry returns `Starting`, and polling starts no further probe after expiry.
An in-progress kernel connection retries interrupted polls against one absolute
connection deadline. `EINTR` neither fails the connection nor renews its budget;
the same socket is retained until readiness, another error or deadline expiry.

## Detached launch

Detached spawn re-executes current_exe with explicit hidden-command arguments,
null stdin, explicit working directory, caller-opened output log and a cleared
environment plus explicit pairs. A pre-exec setsid creates a new session and
process group. This is the sole unsafe site because std has no safe setsid
builder; the callback only calls the async-signal-safe syscall via nix.
The client reaps children while it lives. After client exit, the OS adopts the
detached manager. It is not governed by the client's terminal or process group;
setsid does not promise escape from an externally imposed cgroup/container.

## Registered-service activation

An existing login registration can be activated through `Launcher` instead.
Launchd kickstart and systemd user start adapters do not register or remove
anything. The registration supplies the manager's environment. Products may
implement `Launcher` when activation needs explicit session-bus variables or a
platform-specific registration. Live login registration acceptance is separate
from process-detachment acceptance.

## Acceptance criteria

- Racing starters produce one lease holder; every successful client observes
  that holder's identity.
- A successor can acquire the lease after the holder dies and replace its
  stale owned sockets.
- A detached manager survives parent exit and signals to the parent's group.
- No inspection or readiness probe dispatches an ordinary product request.
- An overall readiness deadline also bounds an in-flight probe.
- One client's readiness wait launches only once even while its child is held
  before lease acquisition.
