# canonic-background-runtime

A Rust library for one background manager per user and data root. It provides
private local IPC, kernel-owned single-instance leases, explicit startup,
bounded ordinary and control lanes, and atomic admission and retirement.
Version-aware clients replace an empty older manager and request retirement
after existing work settles when it is busy.
Startup-authorized clients wait through drain. Opt-in recovery coordinates
stable-ID replay with product reconciliation, dedicated owner channels detect
owner death, and an explicitly enabled force-stop supports crash recovery.

The admission core performs no I/O and accepts a caller-supplied clock. A
synchronous driver uses standard threads; no async runtime is required.
It waits for activity or the next policy deadline, with no periodic idle poll.
Applications supply request handling, product-held obligations, exit policy,
blocker contents and durable request-ID reconciliation.

Explicit connection handshakes transfer a guarded duplex stream after the reply,
freeing request workers while separately bounding live connections. Control
extensions declare inspection, ordinary-work or drain-cleanup roles.

```toml
[dependencies]
canonic-background-runtime = { path = "../background-runtime" }
```

This public library is unpublished. macOS and Linux adapters are implemented;
Linux host acceptance is unverified. Windows endpoint and lease adapters are
unsupported. See [platforms](specs/platforms.md) for the adapter boundary.

- [Get started](docs/getting-started.md): a minimal manager and client.
- [API reference](docs/reference.md): responsibilities, options and errors.
- [Specifications](specs/README.md): contracts, decisions and acceptance map.
- [Development](docs/development.md): individual checks and fixture ownership.
