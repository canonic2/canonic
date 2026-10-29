# Platforms

## Adapter status

| Platform | Implementation | Acceptance boundary |
| --- | --- | --- |
| macOS | Unix sockets, getpeereid, file locks, account-home lookup, setsid, launchd activation | Socket/process acceptance on the local host; live registration not exercised |
| Linux | Unix sockets, SO_PEERCRED, file locks, account-home lookup, setsid, systemd activation | Socket/process fixtures run on a Linux development VM; live systemd activation not exercised |
| Windows | Socket, lease, driver and launch scaffolding; root constructors, peer checks and `supported()` return `Unsupported` until owner and access-list validation is implemented | No fixture or host acceptance has run |

## Windows adapter

Windows runtime use is unavailable: both root constructors and peer checks
return `Unsupported`. Profile placement alone does not establish privacy for
an explicit root or a directory with inherited permissive access. Enabling
Windows requires owner and access-list validation before binding or connecting,
with tests for permissive and foreign-owned roots.

The Windows scaffolding uses the same Unix-domain socket stream type through
`net::Stream`, so framing, deadlines, handoff and clients share the Unix code.
Its adapter boundaries are:

- **Access:** Windows sockets expose no peer credentials through this adapter.
  Root constructors and `check_peer` refuse use until filesystem privacy is
  enforced.
- **Identity:** there is no stable file identity through a safe interface, so
  endpoint cleanup and lease verification rely on the lease and file type alone.
- **Driver:** without a safe readiness poll, each lane has a blocking accept
  thread that enqueues directly into its bounded pool, assigning capacity and
  read deadlines before queueing. Ordinary refusals do not delay control
  acceptance. Only coalesced admission wakeups and accept failures use the
  driver channel, which waits with the policy deadline or no timeout.
  Shutdown unblocks each accept with a connection the stopped thread discards.
- **Connect:** a local connect completes or is refused at once; the connection
  timeout applies only on Unix.
- **Owner EOF:** `owner::Connection::wait` returns `Unsupported` without socket
  I/O or releasing the hold. Enabling monitoring requires repeatable bounded
  waits that do not use blocking receive timeouts on a retained Winsock stream.
- **Launch:** detached spawn uses `DETACHED_PROCESS`, a new process group and
  job breakaway, retrying without breakaway when the job forbids it.
  `Service::ScheduledTask` runs `SystemRoot/System32/schtasks.exe /Run`.
  A missing, empty or non-absolute `SystemRoot`, or a task name starting with
  `-` or `/`, returns `Options` before activation.

## Acceptance boundaries

On macOS, full Unix-listener queues and stale/closed listener paths can both
produce `ECONNREFUSED`. The client retains the conservative leased-endpoint
`Starting` outcome described in [startup](startup.md#inspection-and-readiness).
It cannot promise to distinguish every backlog failure as `Capacity`.

The peer-check test injects a forged UID into the shared comparison and checks
actual same-user peers on real sockets. No real second-user connection has been
run. Activation through real login registrations also remains unverified; tests
must never change the developer's login items as an incidental library check.

Portable core behavior, Unix compilation and real-host behavior are separate
evidence. A successful macOS fixture does not establish Linux socket backlog,
credential, process or service behavior. Registered launch requires separate
acceptance of the OS registration and session environment; a custom launcher
fixture proves only that the library invokes the selected interface.

The package's [verification map](verification.md) names the available fixtures.
Acceptance must use test-owned roots, processes and registrations explicitly
selected for that work, rather than a user's existing manager or login items.
