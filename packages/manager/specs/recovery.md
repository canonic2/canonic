# Failure recovery

## Scope and authority

Process failure and a lost reply are different observations. A manager can die
before dispatch, after performing an effect, or after recording its result.
Restarting the process cannot establish which occurred. The library provides
reconnection and bounded replay orchestration; the product supplies durable
outcome reconciliation and authority to replay.

## Request recovery

`client::call` remains a one-shot exchange. `recovery::call` explicitly allows
startup and a bounded number of sends of the identical request envelope. It
shares one absolute timeout across reconnects, backoff and frame operations,
and verifies product protocol and required capabilities on each ready identity.
The product owns launcher and reconciliation callback execution deadlines.

Definite absence, startup, drain or capacity refusals can reconnect and retry
within that budget. An ambiguous send first reconnects to a ready manager, whose
product initialization has completed under its lease, then invokes the product
reconciliation callback with the unchanged request and current identity.

| Product decision | Behavior |
| --- | --- |
| `Recovered(reply)` | Validate a final result's control version, request ID and final-result marker, then return it without dispatch. |
| `Replay` | Resend the identical ID, protocol, requirements and payload. Product guarantees durable ID/content deduplication or proves the prior attempt had no effects. |
| `Unknown` | Return `OutcomeUnknown` with the original ID; do not replay. |

After the last permitted send, reconciliation can still recover a result but
cannot authorize another send. A recovery deadline or reconnect failure after
ambiguity preserves `OutcomeUnknown`; it is never reported as proof of
non-admission. Reconciliation failures are returned to the caller as errors,
not interpreted as replay authority. Application operations still require
product-owned durable records and content-conflict checks. Blind replay of an
arbitrary mutation is not safe and is never the default.

## Arrival during shutdown

Non-launching inspection reports `Draining` immediately. Connect-or-start with
an explicit launcher waits through drain within its readiness deadline, using
backoff and read-only probes. It can activate a replacement when exclusion is
released and returns only a ready identity. It cannot force a draining holder
to release resources. If drain never settles, readiness expires with `Starting`.
The [update workflow](updates.md) uses this same lease and readiness ordering.

## Owner death

`owner::Connection` pairs a same-user dedicated silent Unix connection with a
product hold. The owning process retains its endpoint while it owns work.
Kernel EOF identifies endpoint closure, including closure caused by process
death, without a PID lookup or a heartbeat expiry. Products must establish this
channel before authorizing the work and bound their retained channels/workers.
The product runtime drives waits; the library creates no monitor threads and
does not scan an inventory on each server tick.

The channel must have one reader and carry no application bytes after setup.
`wait(timeout)` returns false on timeout and true on EOF; unexpected data or
socket errors are errors, not proof of death. EOF is stable on later calls.
All endpoint copies must close for EOF: a descendant that inherits a descriptor
can prolong ownership, and closing an endpoint intentionally is equivalent to
relinquishing ownership. Products select whether disconnect requests cleanup,
cancels work or permits it to continue independently.

Owner death does not prove product resources have been released. The connection
retains its hold after EOF or a wait error. The product starts its resource
cleanup, then drops the connection only once cleanup is established. Uncertain
cleanup remains held. The library cannot infer external resource liveness from
its count, an owner PID or the death of a request's client.

## Explicit force-stop

Products with established crash recovery may opt into
`server::Options::allow_force_stop`, false by default. The server alone owns
advertising `force-stop-v1`; a caller-supplied capability cannot enable it.
The additive control-v1 action `force_stop` requires an exact observed
`manager_id`, null arguments, and should require `force-stop-v1`. Wrong versions,
missing capabilities, stale incarnations, invalid arguments and disabled
support refuse before acknowledgement or termination.

After acknowledging a valid request, the driver attempts a bounded final reply
`{"stopping":true}` and exits the entire process with status 75 even if that
reply is lost. This bypasses worker joins, destructors, product cleanup and live
holds. The kernel releases the lease; stale owned sockets are replaced only
after the successor acquires it. A successful reply is not proof of process
exit: reconnect-or-start still observes lease/readiness ordering.

Force-stop is an explicit crash-recovery operation. Drain, timeout, request
recovery and version replacement never invoke it automatically. It does not
terminate external workers or establish that their resources have stopped.
The product must fence or recover those resources under the successor lease
before readiness. There is no takeover while the old process retains the lease,
and no PID-based signalling fallback if the control lane cannot answer.

## Acceptance

- An uncertain send is never replayed without an explicit product decision;
  a recovered durable result causes no successor dispatch.
- Authorized replay keeps request ID and content unchanged, including after a
  lost admission acknowledgement or final result.
- Actual manager death after recording an effect reconnects to a new process
  and can recover the stored result without repeating the effect.
- A startup-authorized client waits through drain; inspection starts nothing;
  a held draining manager cannot defeat the caller's readiness deadline.
- Killing an owner produces EOF, but retains its hold until product cleanup.
- Explicit force-stop refuses stale incarnations and disabled support, exits a
  draining process with held work, and permits a successor only after lease
  release. Ordinary update and drain behavior remains graceful.

[Verification](verification.md) names the tests and product acceptance boundaries.
