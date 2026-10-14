# Cache artifacts

`ArtifactCache` stores bytes at `<root>/<sha256>`. Choose the root yourself,
typically a user-owned directory shared by your application's projects. Creating
the object performs no I/O; writes create a missing directory.

## Store verified release bytes

```rust
use canonic_registry::{cache::ArtifactCache, catalog::Entry};
use std::error::Error;

fn retain(cache: &ArtifactCache, entry: &Entry, bytes: &[u8])
    -> Result<(), Box<dyn Error>>
{
    cache.put_expected(&entry.sha256, bytes)?;
    Ok(())
}
```

Use `put_expected` when you have a digest from an authenticated, admitted
catalog or validated lockfile. It refuses mismatched bytes. `put(bytes)` computes
and returns a digest for application-owned content; that digest alone establishes
no catalog provenance.

An existing valid copy is kept. A write with the correct bytes repairs a corrupt
or wrongly sized regular entry. Writes use a unique temporary file and atomic
rename, not an in-place modification. On Unix, newly created directories are
0700 and files are 0600.

## Read offline

```rust
use canonic_registry::{cache::{ArtifactCache, CacheError}, lock::LockedPack};

fn retained_bytes(cache: &ArtifactCache, pack: &LockedPack)
    -> Result<Option<Vec<u8>>, CacheError>
{
    cache.get(&pack.digest, 64 * 1024)
}
```

`get` returns `Ok(None)` for a missing root or entry. A present entry is read
within your byte limit and hashed before bytes are returned. Corruption is an
error, not a cache miss. The example uses an application-selected 64 KiB bound.

`contains` checks whether a regular entry exists without reading its bytes.
It does not check integrity or a byte limit. Use `get` before relying on it.

## Failure and storage management

Digest arguments must be 64 lowercase hexadecimal characters. Catalog digests
are normalized when verified; normalize external text with
`digest::normalize_sha256` before passing it to the cache.

Symlink roots, symlink entries and entries of the wrong file kind are refused.
A write does not replace those entries. Refused writes leave existing entries
unchanged, although they may have created a missing root directory.

Files are synced before rename. On Unix, the cache also attempts a directory
sync. If that final sync fails, a cache write still succeeds: after a crash the
entry may need fetching again, and every read verifies its bytes.

The cache provides no eviction, garbage collection or temporary-file cleanup.
An interrupted write can leave a `.<digest>.<process>.<n>.tmp` file; it is never
read as an artifact. Coordinate application cleanup with active writers and
retain digests referenced by installations you need to reproduce offline.

For a corrupt entry, reacquire the exact bytes from a trusted source, require
the locked or catalog digest to match and repair through `put_expected`. See
[Lock and reproduce installs](lockfiles.md) for the cache-miss workflow.

[Documentation index](../README.md)
