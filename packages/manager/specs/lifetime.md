# Admission and lifetime

## States

| State | Contract |
| --- | --- |
| Starting | Initialization is incomplete; ordinary admission is closed. Initial holds may be acquired. |
| Serving | Ready for work; mutations and product resources can own responsibility. |
| Idle | No mutation reservations or holds; the current idle epoch is eligible for policy evaluation. |
| Draining | Mutation admission is closed; admitted work, cleanup and replies must settle. |
| Exiting | All accounted obligations have settled; the driver can join workers and release endpoints. |

Readiness opens `Starting` to `Serving`. Empty policy evaluation can enter
`Idle`; mutation activity or a new hold cancels that idle epoch. Successful
retirement, drain or an exit policy closes admission permanently. A refused
busy retirement restores the prior state.

## Reservations and holds

Admission uses one
short atomic critical section. A reservation precedes any product preflight;
it covers queueing, dispatch, effects and final replies. Initial resources use
holds acquired before readiness. Ongoing resources and workers receive a hold
before their creating request releases its reservation.

The ordinary lane reserves before reading a frame, so queued and incomplete
connections count as work until classified or refused. A decoded observer
converts its reservation to a response obligation. A decoded control extension
must acquire mutation admission before acknowledgement and product dispatch.
Built-in inspection and lifetime controls do not count themselves as mutations.

Dropping a permit settles that request's accounting; it does not prove a
product effect was undone. A product must keep any continuing worker, resource
or uncertain cleanup held independently. An admitted mutation can transfer a
hold during drain. An observer cannot create ownership after closure.

## Retirement and drain

Retirement closes admission, then rechecks work and product holds under the
same lock. A busy decision reopens; a successful decision commits closure.
Observers already decoded can finish but never renew the idle epoch or prevent
closure. After closure no new mutation enters. Every response and held resource
must settle before exiting. Drain closes immediately, invokes product cleanup
outside the lock and waits for admitted work and holds.

Read-only controls can inspect a manager while accepted work or holds remain.
Once those obligations settle during drain, control admission closes under the
same lock that reserves controls. Observer counts cannot reopen it. Already
admitted observers keep only their bounded replies alive; new control arrivals
are refused with `Draining`. Closure and final process exit are distinct: a
successful retirement reply means admission is closed, not that endpoints or
the lease are gone.

`retire_when_empty` records an explicit replacement request. While work or holds
remain, ordinary admission stays open and existing supervision continues. The
next atomic empty recheck closes admission even if the normal policy requires a
grace or explicit stop. The request never forces resource release. A pending
replacement appears in `Snapshot::retirement_pending` and emits one payload-free
`RetirementRequested` event; repeated notifications do not repeat that event.
An automatic exit for that request uses reason `Retired`; an explicit drain
keeps reason `Drained`. Once closure commits, the normal response, endpoint and
lease release order applies.

The [update contract](updates.md) defines which release notifications request
this transition and how clients observe a successor. Pending retirement
transfers no live resources; product state recovery begins under the next
owner's lease.

## Idle policies and clocks

The exit policy receives caller-clock elapsed idle time only when empty.
IdleGrace, WhenEmpty and Explicit demonstrate three policies. Products retaining
stored state use a hold while any such state requires residence. No policy can
override live holds or accepted work. Policy callbacks run outside the lock;
emptiness and the idle epoch are rechecked afterward. Monotonic timestamp
changes never expire live work. Device awake clocks stay outside this library.

| Policy | Empty-manager behavior |
| --- | --- |
| `IdleGrace` | Close once the current idle epoch reaches the supplied grace. |
| `WhenEmpty` | Close on the next eligible empty evaluation. |
| `Explicit` | Wait for an explicit retirement or drain. |

The grace begins when the driver observes a new empty epoch. No policy is a
request deadline or permission to terminate a live resource. Read-only
observations do not restart the grace. If work or holds change while the
policy callback runs, its result cannot close a different idle epoch.

## Exit and lifecycle evidence

The driver joins workers, removes only its own socket identities and releases
the lease last. Products must keep gateway/native cleanup represented by holds
until complete. Failed or uncertain cleanup stays held until product evidence
establishes safe release.

This is the graceful exit contract. The separately enabled
[force-stop control](recovery.md#explicit-force-stop) deliberately terminates
the process without graceful cleanup; successor crash recovery belongs to the
product. Owner EOF detection never silently releases a hold.

Lifecycle evidence is payload-free: ready, idle entered or cancelled, drain
admitted or refused, and exit reason (`Policy`, `Retired` or `Drained`). The
[bounded event ring](limits.md#counts-and-retention) discards oldest diagnostics
on overflow, supports consuming reads and performs no file writes or per-tick
event emission. Dropping diagnostics never drops an obligation.

## Acceptance criteria

- Retirement either refuses busy work without changing admission, or commits
  closure atomically; no later mutation enters.
- Drain preserves admitted replies and responsibility transferred to holds.
- No policy exits while mutation reservations or product holds remain.
- Observers do not renew grace or veto retirement, and continuing control
  arrivals cannot prevent an otherwise settled shutdown.
- Worker joins and owned endpoint cleanup precede final lease release.
