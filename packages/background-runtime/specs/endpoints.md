# Endpoints and peers

An owned socket can be visible between bind and permission restriction. With a
private root and a live kernel lease, clients report that preparation window as
`Starting` and retry without connecting. A foreign endpoint or an owned
permissive endpoint without a live lease remains a private-path refusal.

This topic owns root selection, filesystem privacy, peer identity and socket
cleanup authority. [Startup](startup.md) owns the lease lifetime; the
[security model](security.md) explains the trust boundary.

## Root selection

`Root::for_user` defaults to account-home lookup by effective UID, independent
of `HOME`. Environment-home lookup requires `Home::Environment`. Explicit
roots are resolved to absolute paths; existing parent aliases are resolved once.
Parent traversal components are refused. The root itself and directories
created under it cannot be symlinks. Created directories are
0700, the lease anchor and sockets 0600. Existing permissive or foreign roots
are refused, never silently repaired.

Resolving a root performs no writes. Preparing it creates missing directories;
inspection must use the non-creating path. Existing ancestors can have ordinary
system permissions; the final root must be private and owned by the effective
user. Canonicalizing a parent alias does not authorize a symlink at the root.

## Endpoint layout

`manager.sock` carries ordinary requests; `control.sock` reserves controls.
The full pathname including NUL must fit the platform address
([limits](limits.md#sizes-and-identifiers)). Paths are refused rather than
shortened or hashed.

## Peer checks

Before any frame read, every accepted stream is checked with kernel credentials:
getpeereid on macOS/BSD, SO_PEERCRED on Linux. Clients validate the private
socket and server credentials as well. A forged claimed request identity grants
no authority. Only a matching effective UID is accepted.

## Binding and cleanup

Only the lease holder may remove a stale owned socket and bind. Non-sockets
and symlinks are preserved. Cleanup compares device/inode and validates root
and lease identities; it never removes a replacement endpoint. The same-user
boundary does not isolate mutually hostile processes running as that user.

A lease can have one live endpoint per lane; a second bind is refused without
touching the existing listener. Explicit unlink reports cleanup failures; drop
also attempts cleanup. The persistent lock anchor is never an endpoint cleanup
target. Root or anchor replacement invalidates cleanup authority.

## Acceptance criteria

- Resolving or inspecting an absent root leaves no files or directories.
- A foreign, permissive or symlink root is refused without repairing it.
- A peer is checked before reading its first frame.
- Only the lease holder binds or replaces stale owned sockets.
- Closing an endpoint preserves any replacement socket and leaves the lease
  held until its owner releases it.

See [verification](verification.md) for coverage and the real second-user gap.
