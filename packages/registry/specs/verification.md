# Verification

## Checks

`make check` runs every gate with all features enabled:

| Gate | Command |
| --- | --- |
| Formatting | `cargo fmt --check` |
| Lint | `cargo clippy --locked --all-targets --all-features -- -D warnings` |
| Tests | `cargo nextest run --locked --all-features` |
| Doctests | `cargo test --doc --locked --all-features` |
| API docs | `cargo doc --locked --all-features --no-deps --document-private-items` |
| Dependencies | `cargo deny check` (advisories, licenses, bans, sources) |

Lint policy lives in `Cargo.toml`, with the test exemption in `clippy.toml`:

- `unsafe_code` is forbidden, and `unreachable_pub` warns.
- Clippy pedantic, including `too_many_lines`, applies everywhere.
- `unwrap_used` applies outside tests.
- `mod_module_files` enforces self-named modules.
- rustdoc denies broken and private links.

A justified exception is a narrow `#[expect(..., reason = "...")]`. Coverage
is a report (`make coverage`) with no percentage gate.

## Test layout

**Unit tests** live with each module, or in its `tests.rs` submodule. They
cover:

- validation and signatures, catalog lookups and selection, checkpoints,
  references and installed matching;
- resolution, including a property test that resolved closures satisfy every
  requirement;
- registrations and the global file, with real temporary files;
- the cache, local acquisition and the shared file helpers;
- the HTTPS transport's setup and request deadline layers;
- hexadecimal round trips and timestamp ordering, as property tests.

**One integration binary**, `tests/integration/main.rs`, requires the
`test-support` feature and exercises the public API against owned TLS servers:

- **Transport:** certificate and host validation, refused redirects,
  response bounds, encoded and failed responses, bodies cut short, deadlines,
  and unsafe paths refused before any request.
- **Trust:** first-use discovery, an impostor key, one identity for every
  spelling of host and key, enrolled keys not being replaced, expiry, a
  missing registry ID, rollback and release replacement.
- **Workflow:** a complete install across a signed global registration and a
  local project registration with a dependency between them. It caches the
  bytes, checks a reviewed lockfile snapshot, and reproduces the install
  after both registries are gone. A source that is not registered refuses
  resolution.

Fixtures use Acme and example.com names, temporary directories and ephemeral
loopback ports. HTTPS tests open loopback listeners, so in an agent sandbox
they need host network access.

## Acceptance

- **Format:** a catalog with an unknown field, an invalid release, a
  duplicate release or too many releases is refused whole.
  Catalog verification is bounded before any signature or parse work.
- **Trust:**
  - A catalog changed by one byte, signed by another key, expired, naming
    another registry or missing its sequence is refused.
  - Expiry is decided correctly for clocks before 1970 and after year 9999.
  - A registration whose source claims another source's identity is refused,
    whether added or read from a file.
  - Every spelling of a signed source's host, default port and key, typed or
    served at discovery, yields one identity; a stored source not in
    canonical form is refused.
  - A published key that does not sign its catalog is not accepted at
    discovery, and discovery reviews the exact bytes it authenticated.
  - An enrolled key is never replaced by the network.
  - Rollback and release replacement are refused, with checkpoints unchanged.
  - A local source may move from unsequenced to sequenced catalogs, but cannot
    omit its sequence afterward, including after checkpoint serialization.
    Refusing that transition preserves sequence, catalog digest and releases;
    identical catalogs can be readmitted and higher sequences still advance.
  - A catalog is admitted only under the identity of the source that
    authenticated it.
  - A release removed from a catalog keeps its retained digest, and the
    checkpoint bound refuses only new sources.
- **Acquisition:**
  - Local reads cannot escape their directory, refuse a FIFO without
    blocking, and refuse a source that moved, was deleted, was replaced by a
    file or became a link as an identity change.
  - On Unix, a link in place of a file that was checked or canonicalized is
    refused when it is opened.
  - The largest possible size limit does not overflow.
  - HTTPS reads refuse untrusted certificates, a mismatched host, redirects,
    encodings, non-200 statuses, oversized bodies, bodies cut short and late
    responses, each with its specified refusal.
  - No host can keep a read alive past its deadline: a trickled body, a
    gapless flood of chunked trailers, and a TLS record dripped a byte at a
    time all end at the deadline.
  - A dripped TLS handshake ends at the setup deadline, however far away the
    overall deadline is; an established connection may then take longer.
  - Artifacts with another digest are refused.
- **Registrations:**
  - The built-in alias stays reserved and unshadowed, and combining scopes
    refuses one that bypassed validation.
  - Global writes are atomic under a lock, and refuse stale reviews, links,
    paths that collide with their lock or temporary file, and values that
    would not fit the file bound, leaving the previous file readable.
  - An error after the commit point is reported as committed, never as an
    unchanged file, and nothing is rolled back. A lock that cannot be removed
    is reported with whatever else happened.
  - Defaults must name a registration.
- **References:**
  - Each reference form resolves as specified.
  - An ambiguous bare name is refused with every matching `publisher/name`.
  - An ambiguous installed reference is refused.
- **Resolution:**
  - The highest compatible closure is chosen whatever the input order.
  - Pins and prerelease rules hold, and levels propagate.
  - Candidates with cycles, unenrolled dependencies or root-level conflicts
    backtrack to compatible older releases; exact pins to them refuse.
  - Diagnostics return the same selections and failures as ordinary resolution,
    have deterministic ordering and describe branches rather than globally
    unusable releases. A rejected release may succeed under other choices.
  - Exhausted closures, unenrolled roots, exceeded bounds and malformed
    level lists refuse. Reporting does not relax closure or search bounds.
- **Installed state:**
  - Cached artifacts are verified on every read, a linked cache directory is
    refused by reads and writes, and writes never remove a temporary or other
    file they did not create, replacing only a corrupt cache entry.
  - A corrupt or wrongly sized cache entry is repaired by storing its true
    bytes; a write whose directory sync fails is still stored. Cache files
    are 0600 in a 0700 directory.
  - Lockfile entries carry no machine paths, bound their requirements and
    validate their closure.
- **Workflow:** an installation across a signed and a local registry, with a
  dependency between them, is reproduced from its lockfile and cache after
  both registries are gone.
