# Process Kit

Run a shared local background manager for your application's clients.

`canonic-process-kit` is a Rust library for applications whose commands need to
share long-lived work. It provides private local IPC, single-instance leases,
explicit startup, request admission and coordinated shutdown. Your application
supplies the request handler and decides which resources keep the manager alive.

## Add the library

Process Kit is unpublished and used from source. It supports macOS, Linux and
Windows 10 version 1803 or later. Consumers require Rust 1.97 or newer; the
checkout pins its own development toolchain.

From a sibling Rust package, add this to `Cargo.toml`:

```toml
[dependencies]
canonic-process-kit = { path = "../process-kit" }
serde_json = "1.0" # Request/reply bodies in the manager/client example
```

Adjust the path to your checkout's `packages/process-kit` directory.

## Start a manager and client

Follow [A minimal manager and client](docs/getting-started.md) to build a program
with an internal manager mode and an ordinary client mode. The example starts
an absent manager explicitly, sends an echo request and prints
`{"echo":"hello"}`. The manager exits after its empty grace period.

Inspection can connect without starting or preparing a manager. Clients that
may start work choose a launch path explicitly. For work that outlives a reply,
keep a resource hold until cleanup finishes so shutdown waits for it.

## Lifecycle and recovery

The admission core performs no I/O and accepts your clock. The synchronous
driver uses standard threads, without an async runtime or periodic idle poll.

Version-aware clients can replace an empty older manager; a busy older manager
retires after its work settles. Optional recovery supports owner-death detection,
stable-ID replay and explicit force-stop. Your product still owns durable
request-ID reconciliation and the meaning of a completed operation.

For live takeover, [build a planned handover](docs/live-handover.md) with reversible
admission quiescence, irreversible fencing and bounded file/socket copies. Your
application coordinates durable ownership and adoption; automatic version retirement
does not migrate running work.

- [API reference](docs/reference.md): request handling, resource holds, control
  lanes, version handoff and recovery.
- [Get started](docs/getting-started.md): the complete manager/client example.

## Development

See [Development](docs/development.md) for individual checks and fixture
ownership. Tests and socket fixtures use their own temporary roots.

## License

Process Kit is subject to the [Canonic Proprietary License](LICENSE).
Third-party components retain their own licenses and notices.
