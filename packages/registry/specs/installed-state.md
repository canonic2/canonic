# Installed state

After an installation is approved, a product keeps two things: the verified
artifacts, so installed packs work offline, and a lockfile, so every machine
installs the same releases. The library owns the artifact cache and the
lockfile entries. The product chooses where they live and owns the lockfile
document. Checkpoints, the third kind of retained state, are owned by
[trust](trust.md#checkpoints).

## Artifact cache

The cache keeps verified bytes in one directory, usually per user and shared
by every project.

- Each artifact is stored once, named by its SHA-256, and never changed in
  place.
- Storing bytes names them by their digest. Storing them under an expected
  digest refuses bytes that do not match. An existing valid copy is kept;
  storing the true bytes over a corrupt or wrongly sized regular file
  repairs it.
- Writes are atomic and durable: bytes go to a uniquely named temporary file,
  which is synced and renamed into place, and on Unix the directory is then
  synced. A refused write leaves every entry unchanged; it may have created
  a missing cache directory. If only the directory
  sync fails, the entry is still stored and the write succeeds: reads verify
  it, and losing it in a crash only means fetching it again. On Unix, files
  are 0600, and a cache directory and any missing parents the library
  creates are 0700.
- Each sync, of a file or of its directory, is `fsync` on macOS and Rust's
  `File::sync_all` elsewhere. On macOS the library does not issue
  `F_FULLFSYNC`, which also empties the drive's write cache and waits behind
  every other process's flush on the host.
- Reading an artifact returns nothing when it, or the cache directory, does
  not exist. It requires a regular file within the reader's size limit, and
  verifies the digest; a mismatch is reported as corruption and the bytes are
  never returned.
- Whether an artifact is cached can be checked without reading it.
- A cache directory or entry that is a symbolic link, or an entry that is not
  a regular file, is refused by every operation, reads, checks and writes
  alike. A write never replaces such an entry. On Unix, reads open the entry
  without following a link, so an entry swapped for a link after it was
  checked is still refused.

| Refusal (`CacheError`) | Cause |
| --- | --- |
| `Digest` | The name is not 64 lowercase hexadecimal characters |
| `NotRegular(path)` | The directory or entry is a link or the wrong kind of file |
| `Corrupt(digest)` | The stored bytes no longer match their name |
| `Mismatch` | Bytes differ from the expected digest |
| `TooLarge` | The stored file exceeds the reader's limit |
| `Io` | A filesystem failure |

## Lockfile entries

A product's lockfile embeds one entry per installed pack:

| Field | Meaning |
| --- | --- |
| `selector` | The installed [selector](references.md#installed-selectors) |
| `registry` | Source identity of the release |
| `id`, `version` | `publisher/name` and exact version |
| `digest` | Lowercase SHA-256 of the artifact |
| `pinned` | An exact version was requested; updates leave it unchanged |
| `direct` | The operator selected it, rather than a dependency |
| `dependencies` | The release's dependencies, each with `registry`, `pack` and `requirement`; omitted when there are none |

Requirements:

- Entries hold no machine paths, aliases or user identities, and unknown
  fields are refused, so one installation produces identical entries on
  every machine.
- Which packs depend on an entry is derived from the dependencies, never
  stored twice.
- Entries have a reproducible order: by source identity, then publisher,
  then name.
- A set of entries is a valid closure when:
  - the number of entries is within its [bound](limits.md#installed-state);
  - selectors are unique, and so are source and `publisher/name` pairs;
  - digests and identities are well formed, and requirements are
    [bounded](limits.md#text-and-identifiers);
  - every dependency targets a locked pack whose version satisfies it;
  - every entry is reachable from a direct entry.
- Reproducing an installation fetches a missing artifact from its recorded
  source by `publisher/name` and version. The product must refuse a catalog
  entry whose digest differs from the locked digest. Reading the artifact
  then checks the bytes against that digest.

| Refusal (`LockError`) | Cause |
| --- | --- |
| `Limit` | Too many entries |
| `Duplicate(label)` | A repeated selector, or source and pack pair |
| `Malformed(label)` | A malformed digest or source identity, or an oversized requirement |
| `MissingDependency(label)` | A dependency targets a pack that is not locked |
| `Unsatisfied(label)` | A locked version does not satisfy a dependency's requirement |
| `Unreachable(label)` | A dependency entry is not reachable from any direct entry |

## Decisions

- **One cache per user, addressed by digest.** Identical bytes are stored once
  across projects, and a name is its own integrity check.
- **The product owns the lockfile document.** Products record different
  extra state (outputs, levels, aliases), so the library standardizes only the
  entries that describe installed packs.

## Gaps

- A write interrupted by a crash leaves its temporary file,
  `.<digest>.<process>.<n>.tmp`, in the cache directory. Nothing removes these
  files. They are never read as artifacts.
- The cache directory is checked before each operation but not held open,
  so a directory replaced by a link between that check and a read or write
  is followed; only the entry itself is opened without following links.
