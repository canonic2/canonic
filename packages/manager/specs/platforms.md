# Platforms

## Adapter status

| Platform | Implementation | Acceptance boundary |
| --- | --- | --- |
| macOS | Unix sockets, getpeereid, file locks, account-home lookup, setsid, launchd activation | Socket/process acceptance on the local host; live registration not exercised |
| Linux | Unix sockets, SO_PEERCRED, file locks, account-home lookup, setsid, systemd activation | Implemented behind cfg; host execution unverified |
| Windows | Platform-neutral core, frames and envelope types | Root and lease constructors, `supported()` and spawn/service calls return typed Unsupported; Unix client/server driver unavailable |

## Windows adapter requirements

A Windows adapter must supply an owner-only named pipe and LockFileEx lease.
It can drive the same core with its own endpoint guard borrowing a live lease,
peer/user check, bounded byte frames and readiness probes. No Unix pathname,
descriptor or process-group assumption appears in the core or envelopes.
Windows adapter implementation and host acceptance are not established.

## Acceptance boundaries

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
