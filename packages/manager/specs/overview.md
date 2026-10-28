# Overview

## Purpose

Short-lived clients sometimes need a shared process to retain live resources
between requests. Starting one process per client loses that ownership;
sharing one without a lifetime contract risks duplicate managers, dropped
work or a process that never exits. `canonic-manager` provides the common
startup, IPC, admission and shutdown mechanics.

One manager serves one effective user and one selected root. An account-home
default root is shared by all clients; an explicit root is independent.
The library owns mechanics, not resource semantics or CLI naming.

## Workflow

1. Select a [root](endpoints.md#root-selection) without creating anything.
   A product chooses the relative default location or supplies an explicit root.
2. [Inspect or connect](startup.md#inspection-and-readiness). Inspection never
   launches. A caller that explicitly supplies a launcher may start a manager
   and wait for an identity reply.
3. The manager acquires the [kernel lease](startup.md#single-instance-lease).
   The product initializes its state under that lease and acquires holds for
   initial resources before the driver binds endpoints and declares readiness.
4. The client checks the product protocol and required capabilities, then sends
   one [request](protocol.md). The driver reserves admission before preflight,
   acknowledges acceptance and retains the reservation through the final reply.
5. The product transfers any responsibility that outlives the request to a
   [hold](lifetime.md#reservations-and-holds). A lost reply is reconciled through
   the product's durable outcome records, without transport replay.
6. An idle policy or an explicit retirement closes admission. A drain also
   asks the product to begin cleanup. Accepted work, replies and held resources
   settle before endpoints are removed and the lease is released.
7. An explicitly versioned connection can request an older manager's
   [replacement](updates.md). Busy work keeps its owner; a successor acquires
   the released lease and recovers product state before becoming ready.

## Vocabulary

| Term | Meaning |
| --- | --- |
| Root | The resolved directory selecting one independent manager instance. |
| Lease | Kernel-owned exclusion on the persistent lock anchor for that root. |
| Incarnation | The identity of one manager process lifetime, distinct from its build or protocol version. |
| Reservation / permit | An admitted request's responsibility, retained through final reply publication. |
| Hold | Product-owned responsibility for a resource, worker, owner channel or uncertain cleanup beyond one request. |
| Observer | A read-only response that must finish but does not renew the idle epoch or veto retirement. |
| Lane | A bounded worker pool and queue for ordinary connections or reserved controls. |
| Empty | No mutation reservations and no product holds; observer replies may remain. |
| Retirement | Close admission only if empty, preserving a busy manager. |
| Pending retirement | Remember a replacement request while busy, then close on an atomic empty recheck. |
| Drain | Close mutation admission immediately and begin product cleanup, preserving accepted obligations. |

## Responsibilities

The library owns private endpoints, peer checks, lease mechanics, explicit
launch and readiness, bounded framing and lanes, independent controls, atomic
admission, and responsibility accounting. The core performs no I/O and takes
time from its caller; the synchronous driver supplies a monotonic clock.

Products own requests, authorization, durable records, stable-ID deduplication,
resource handles, blocker contents, cleanup, exit policy and login registration.
They also own state-format compatibility and recovery across manager updates.
An unknown resource lifetime must be represented by a hold until the product
proves release. Retained records alone need not hold a manager alive.
The [recovery contract](recovery.md) provides bounded replay orchestration,
owner EOF detection and optional explicit force-stop; products supply durable
replay authority, resource cleanup and successor crash recovery.

Inspection does not create directories, take a writer lease or start a process.
Stop and drain against absence can be reported as stopped by the product CLI.
The library exposes functions rather than command parsing.

## Status

The admission core, Unix adapters and synchronous driver are implemented.
This is an unpublished public library. [Platforms](platforms.md) records host
acceptance boundaries; [Verification](verification.md) records test coverage
and implementation gaps. Generic echo fixtures do not establish a product's
resource ownership, recovery or durable mutation correctness.
