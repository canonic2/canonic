# Limits

This topic owns numeric bounds and their meanings. Exceeding a bound is a
refusal, never truncation. The public [reference](../docs/reference.md#bounds)
describes configuration for integrators; this specification also records
validation rules and implementation gaps.

## Sizes and identifiers

| Bound | Value and owner | Failure behavior |
| --- | --- | --- |
| Frame payload | 1 MiB default; caller-selected in byte helpers and client options | `FrameTooLarge` before allocating an oversized body or writing a prefix. |
| Server frame payload | `Options::frame_limit`, 1,024 through `u32::MAX` bytes | Invalid configuration: `Options`; oversized input or result: `frame_too_large`. |
| Request and manager ID | 1–128 ASCII bytes: letters, digits, hyphen or underscore | Invalid request ID refused; invalid server identity rejects startup. |
| Server build string | At most 128 bytes | Invalid identity rejects startup. |
| Ordered version label | At most 128 bytes, complete semantic version | Opt-in version checks return `InvalidVersion`; no retirement for unknown order. |
| Server capability name | Same syntax and length as an ID | Invalid identity rejects startup. |
| Full Unix socket path, including NUL | 104 bytes on macOS; 108 on Linux | `PathTooLong`; no hashing or shortening. |

JSON encoding is bounded before frame publication. A result that cannot fit
is replaced with a bounded error carrying the original request ID and
incarnation. Opaque payloads, arguments and product error data share the frame
budget; they do not have separate library size allowances.

## Counts and retention

| Bound | Default / maximum | Owner and behavior |
| --- | --- | --- |
| Ordinary workers | 8 / no more than ordinary capacity | `server::Options::workers`; must be positive. |
| Ordinary connections, active plus queued | 24 / 65,536 | `capacity`; excess connections refused. |
| Control workers | 1 / no more than control capacity | `control_workers`; must be positive. |
| Control connections, active plus queued | 9 / 65,536 | `control_capacity`; excess connections refused independently. |
| Server capabilities | At most 128, including the library's version-retirement capability when available | Invalid identity rejects startup. |
| Live bound endpoints per lease | One per lane | Duplicate bind returns `EndpointBound`. |
| Retained lifecycle events | Latest 64 | Oldest event discarded on overflow; `events()` consumes retained entries. |

Unread connections occupy lane capacity. Queues and workers share the capacity
budget; the queue is not an additional allowance. Product holds are accounting
tokens, not a library resource quota. Products bound resources, long-lived
channels, blocker pages and callback memory themselves. The library keeps no
durable request history, deduplication journal or product inventory.

## Time

| Bound | Default / schedule | Meaning |
| --- | --- | --- |
| Server I/O | 3 seconds, `io_timeout` | One total queued-frame read budget from enqueue; each worker reply write gets a fresh bounded budget. |
| Accept-thread refusal writes | At most 20 ms, capped by a shorter `io_timeout` | One absolute write deadline for an enqueue refusal, including every partial write and any bounded encoding fallback; independent of worker reply budgets. |
| Client transport | 3 seconds, `io` | Connection timeout and per-frame read/write deadline; readiness probes cap each phase by the same absolute readiness deadline. |
| Overall client readiness | 15 seconds, `readiness` | Remaining readiness time must bound each probe's transport operations. |
| Recovery sends and elapsed I/O | Caller-selected positive `recovery::Options::attempts` and `timeout` | First send counts; each replay requires product authority after ambiguity. One absolute timeout caps reconnects, backoff and frame operations. |
| Lease wait | Caller-selected; zero permits one immediate attempt | Expiry while held returns `AlreadyRunning`. |
| Lease and readiness backoff | Starts at 2 ms, doubles up to 100 ms | Sleep is capped by remaining wait time. |
| Server listener wait | Next policy deadline, or indefinite without a timer | Listener activity and coalesced admission notifications wake the loop; fractional milliseconds round up and the OS poll limit caps very long waits. |

Server I/O timeouts must be nonzero. Endpoint connection rejects a zero
transport timeout. Time uses a monotonic clock; the admission core receives
timestamps rather than reading a clock. An I/O deadline does not cancel a
product callback. Products own handler deadlines, cleanup completion and
custom launcher duration.
Recovery callbacks also own their execution deadline. Owner-channel waits
take a caller-selected timeout without treating expiry as owner death.

Partial reads or writes must consume the same frame budget, rather than renew
it. A queued connection that exhausts its read deadline must not dispatch.
Pure `frame` helpers rely on the supplied reader or writer to enforce time.

## Verification gap

Client request writes use an absolute frame deadline, capped by readiness during
probes. A real trickled-request-write regression is missing.

The [idle scheduling contract](lifetime.md#scheduling-and-notifications) forbids
fixed periodic listener polling. No package-wide RSS or callback-latency budget
is established. Capacity and frame bounds do not by themselves bound memory
allocated inside products.
