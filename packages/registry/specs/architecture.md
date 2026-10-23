# Architecture

One Cargo package, `canonic-registry`, with a library and no binary. Rust
1.99.0 and Rust 2024 are pinned. Capability modules are self-named files
directly under `src/`. A module's adapter or larger parts live in self-named
submodules, and its tests move to a `tests.rs` submodule once they outgrow it.

## Modules

Domain modules hold pure rules and types:

| Module | Owns | Spec |
| --- | --- | --- |
| `digest` | SHA-256, digest normalization and fixed-length hexadecimal | — |
| `timestamp` | UTC parsing and formatting for catalog expiry | [format](registry-format.md#catalog) |
| `names` | Identifiers, `PackId`, text and source-identity bounds, and safe artifact paths | [format](registry-format.md#catalog), [limits](limits.md#text-and-identifiers) |
| `source` | `Source` identity and HTTPS location rules | [trust](trust.md#source-identity) |
| `catalog` | Catalog schema, signature verification, authentication and lookups | [trust](trust.md#catalog-authentication), [references](references.md#catalog-lookups) |
| `checkpoint` | Rollback and replacement refusal | [trust](trust.md#checkpoints) |
| `registrations` | Aliases, scopes and `Effective` | [registrations](registrations.md) |
| `selector` | References, installed selectors and installed matching | [references](references.md) |
| `resolution` | Dependency resolution and the `Level` trait | [resolution](resolution.md) |
| `lock` | Lockfile entries and closure checks | [installed state](installed-state.md#lockfile-entries) |

Adapters perform I/O for one capability each:

| Module | Owns | Spec |
| --- | --- | --- |
| `acquisition` | `Fetcher`: local reads, catalogs, artifacts and discovery | [acquisition](acquisition.md), [trust](trust.md#discovery) |
| `acquisition::hosted` | The bounded HTTPS agent and request, the socket layer between TCP and TLS that enforces each request's setup and overall deadlines, and the layer above TLS that marks the connection established | [acquisition](acquisition.md#reads) |
| `registrations::global_file` | `GlobalFile`: validated reads and locked replacement | [registrations](registrations.md#global-file) |
| `cache` | `ArtifactCache` | [installed state](installed-state.md#artifact-cache) |

Shared infrastructure and test support:

| Module | Owns |
| --- | --- |
| `files` (private) | Bounded reads, entry kinds without following links, private directories and atomic private writes, used by every filesystem adapter |
| `testing` | Public test support ([below](#test-support)) |
| `test_support` (unit tests only) | Fixtures shared by the library's own unit tests: example keys, sources, releases and catalogs |

## Dependency direction

Domain modules perform no filesystem, network or process I/O and never depend
on adapters. Adapters depend on domain modules and on `files`, never on each
other's internals; `files` depends on nothing in the crate. `testing` depends
on `acquisition` to build fetchers that trust only its fixture CA.

## Conventions

- **Errors:** each capability returns its own `thiserror` enum, because
  consumers branch on the kind of failure, such as rollback, expiry or an
  unenrolled dependency. Each topic lists its error variants. Acquisition
  wraps catalog and source errors transparently. `files` returns
  `std::io::Error` and outcome enums (`Bounded`, `Commit`) that each
  adapter maps to its own error.
- **Time:** functions that check expiry take `now`. The library never reads
  the wall clock; HTTPS deadlines are measured on the monotonic clock.
- **State:** checkpoints, registrations and lockfile entries are values the
  caller persists. Only `GlobalFile` and `ArtifactCache` write files, and
  only under paths the caller supplies. Contents are written through
  `files`; `GlobalFile` also creates and removes its own lock file and
  removes a stale temporary file.
- **Bounds:** every input is bounded ([limits](limits.md)).
- **API:** public items are deliberate and documented. Every fallible public
  function documents its errors.
- **Stored values:** every stored type refuses unknown fields. Identity types
  (`Source`, `PackId`, `InstalledSelector`) are validated again when they are
  deserialized. Aggregates are checked by their own rules after loading:
  `GlobalFile` validates the global scope it reads, and `Registrations::validate`
  and `lock::validate` check project scopes and lockfile entries.
- **Dependencies:** `ring` (Ed25519, SHA-256), `semver`, `serde`,
  `serde_json`, `thiserror`, and `ureq` with Rustls and public roots; its
  re-exported `http::Uri` also parses locations in `source`, without I/O.
  On Unix, `libc` supplies the non-blocking and no-follow open flags; it is
  already part of the graph through `ring`. On macOS, `rustix` supplies a
  safe `fsync` for [durable writes](installed-state.md#artifact-cache),
  since the crate forbids unsafe code.
- **Robustness:** no input can make the library panic, overflow or block:
  slicing and indexing are guarded by prior validation, size arithmetic
  saturates, and on Unix files are opened without blocking.

## Test support

`testing` is compiled for the library's own unit tests, and for consumers
when they enable the `test-support` feature, which they do only in their
dev-dependencies. It always provides:

- `SigningKey`: a generated Ed25519 key that produces `catalog-key.pub` and
  `catalog.sig` content.

With the feature, it also provides:

- `Server`: an owned localhost HTTPS server serving configured `Response`s
  and recording request paths. A response sets its status, body, encoding and
  delay, and can misbehave on purpose: announce more than it sends, trickle
  its body, or follow an empty chunked body with endless trailers. It stops and
  joins its worker on drop.
- `Server::fetcher(deadline)`: a `Fetcher` that trusts only the fixture CA.
- `SignedRegistry`: a server plus key that publishes releases and signed
  catalogs with a chosen sequence and expiry, or exact catalog bytes.

The fixture certificates are public test data valid from 2026 to 2126; the
CA's signing key is not retained. The feature adds `rustls` for the server
only. Production code never enables it.
