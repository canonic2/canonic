# Architecture

## Layers and ownership

| Module | Layer | Owns |
| --- | --- | --- |
| `core` | I/O-free core | Admission, permits, holds, lifecycle, exit policies and events. |
| `protocol` | I/O-free protocol | Envelopes, identity, capability checks and refusal codes. |
| `version` | I/O-free core | Semantic release precedence, excluding build metadata. |
| `frame` | Byte driver | Length-prefixed reads and writes, bounded JSON encoding. |
| `endpoint` | Host adapter | Root resolution, private paths, peer credentials, lease and endpoint guards. |
| `launch` | Host adapter | Detached re-execution and existing-service activation through `Launcher`. |
| `client` | Orchestration | Non-launching inspection, explicit startup, readiness, one-shot exchanges, connection handshakes and upgrade probes. |
| `recovery` | Orchestration | Bounded reconnect and identical-envelope replay under product reconciliation authority. |
| `owner` | Host adapter | Dedicated owner-channel EOF detection while retaining a resource hold. |
| `connection` | Host adapter | Prepared handoff receiver, product-owned stream, capacity slot and lifetime guard. |
| `server` | Synchronous driver | Ordinary/control pools, transport deadlines, dispatch and shutdown order. |
| `wakeup` | Private host adapter | Coalesced poll notifications and retained thread wake tokens for cleanup. |
| `unsupported` | Platform boundary | Typed endpoint and lease refusal where adapters are unavailable. |

Dependencies point inward: client and server coordinate the core and adapters;
the core and envelopes depend on no platform adapter or runtime. Framing accepts
generic readers and writers without choosing how connections are established.

## Threads and shared state

The server thread owns both listeners and coordinates lifetime checks. Each
lane has a bounded queue and worker threads. Workers hold a request's permit
through decoding, dispatch and the final reply attempt; lane capacity counts
queued and active connections. A shared receiver lock covers dequeue only,
never callback execution or reply writing.

The core's mutex protects only counters, timestamps and the
[bounded event ring](limits.md#counts-and-retention). It encloses no filesystem,
stream, subprocess or product callback.
The only whole-inventory work occurs on explicit product blocker requests.
The server waits on two listener descriptors and a coalesced wake socket, using
the next policy deadline or no timeout ([scheduling](lifetime.md#scheduling-and-notifications)).
The core holds a runtime `Waker` and invokes it only after releasing its lock;
the host adapter supplies notification I/O. No idle timer runs while work or
holds pin the manager. The driver never polls durable records or enumerates
product resources. A hot ordinary backlog must not starve the control lane or lifetime
evaluation; the loop accepts at most one connection per lane per turn.

## Resource ownership and callbacks

An endpoint borrows its lease, structurally preserving release order. Workers
retain reservations through reply writing; product workers and owner channels
retain holds independently. Handler callbacks are the real extension boundary.
`Launcher` has detached-spawn and registered-service implementations.

`Handler` owns product dispatch, observer classification, extension control roles,
blocker summaries and the start of drain cleanup. Classification and preflight
run under a reservation but outside the core lock. Control callbacks must
return promptly or transfer slow work to a product worker with a hold. Cleanup
cannot wait on a worker whose progress requires that same callback to return.

`Handler::open` prepares a `connection::Handoff`. The synchronous driver writes
the final reply before invoking its receiver with a guarded stream. The receiver
must promptly transfer ownership to product scheduling; it must not run a
long-lived session on the request worker. No per-session thread is created by
the library. `Connection::into_parts` separates the socket from its guard for
runtime adapters; the guard outlives every cloned socket and resource cleanup.

Callback panics are caught by workers so other admitted replies are not
abandoned. Catching a panic does not establish rollback or release product
resources. After a listener/poll failure the driver closes admission, requests
cleanup and waits for accounted obligations before joining workers. Endpoint
cleanup errors remain reportable; the library cannot invent cleanup authority.

The explicitly enabled [force-stop path](recovery.md#explicit-force-stop)
terminates the entire process instead of following graceful shutdown. It is
disabled by default and requires product crash recovery.

## Alternative drivers

An async adapter can use the same `Admission`, `Permit`, `Hold`, `Identity`,
envelopes and `Root`/`Lease`/`Endpoint` without the thread driver. It must reserve
before preflight, apply total frame deadlines, retain the control lane, and
release endpoints before the lease. No async runtime dependency is introduced.
Register the runtime's waker before observing admission, use `next_check` for
its timer, and retain wake notifications racing that observation. Every custom
exit policy implements both `exit` and `next_check`.

Reusing core types does not provide the synchronous driver's scheduling or
validation automatically. An alternative driver owns its queue, observer
classification, response completion, deadline enforcement and acceptance tests.

## Engineering conventions

The package uses Rust 1.99.0, edition 2024, self-named module files and a pinned
Cargo.lock. Public fallible APIs use typed errors and document failure cases.
Unsafe code is denied except for the documented pre-exec `setsid` call in
`launch`; that callback performs only the async-signal-safe syscall. There is
no runtime, logging, product storage or login-registration initialization on
library import. Package check entry points are documented in
[development](../docs/development.md).
