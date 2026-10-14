# Errors and troubleshooting

Errors are typed refusals. Preserve the operation, source identity and pack
context in your application so an operator can distinguish a network problem
from invalid content or a committed state write. Exact variants are listed in
the [generated API reference](api.md).

## Acquisition and catalogs

| Error | What to check |
| --- | --- |
| `AcquisitionError::Location` or `SourceError::Url` | Existing local directory or supported HTTPS base URL; constructors require a trailing slash |
| `LocalIdentityChanged` | The enrolled local root is gone, is not a directory, or canonicalizes to a different path |
| `Path` or `Escapes` | Safe relative path, regular file and containment within the local source |
| `Transport` | Public TLS validation, connectivity, overall timeout and the 3-second setup deadline |
| `Response` | HTTP 200, no redirects and no `Content-Encoding` |
| `TooLarge` | Catalog, trust-file or caller artifact byte limit |
| `Key` or `RegistryId` | Discovery key encoding and signed catalog identity metadata |
| `Digest` | Artifact bytes differ from the supplied catalog entry's SHA-256 |
| `Io` | Local filesystem permissions, missing files or other I/O failures |

`CatalogError` is returned by `Catalog::verify` and wrapped by
`AcquisitionError::Catalog` during fetching:

| Catalog error | Recovery |
| --- | --- |
| `SignatureMissing` or `SignatureInvalid` | Restore a signature over the exact served bytes using the enrolled key |
| `Identity` | Serve the enrolled registry ID; changing source identity needs explicit enrollment |
| `SequenceMissing` | Include a non-negative sequence in the signed catalog |
| `ExpiryInvalid` | Supply a valid whole-second UTC expiry |
| `Expired` | Publish a renewed catalog with a higher sequence; also check the caller's clock |
| `Malformed`, `DuplicateRelease` or `InvalidEntry` | Correct schema, unknown fields, duplicate releases, paths or digests |
| `Limit` | Reduce document size or listed releases to supported bounds |

A publisher deployment can briefly expose a mismatched catalog/signature pair.
Retry after the deployment becomes consistent; do not bypass signature checks.
An expired catalog prevents new signed acquisition, but cached installed bytes
can still be read offline.

## Checkpoints

`CheckpointError::Rollback` means a lower sequence, changed bytes at the same
sequence, or an omitted sequence after the source started sequencing.
The publisher must restore the accepted bytes or publish a valid
higher sequence. `Replaced(release)` means that release was assigned a different
digest; publish the new content under a new version instead.

`Limit` means checkpoint source or cumulative release retention is full. The
library has no automatic pruning API. Treat resetting retained state as an
explicit application trust decision: discarding it discards rollback and
replacement history. Refused admissions leave the previous value intact.

## References and resolution

For `SelectorError`, check reference length, segment syntax and the exact
`@version`. `LookupError::Missing` means the catalog has no matching pack;
`Ambiguous` lists qualified alternatives. `Catalog::select` returning `None`
means the exact version is absent or no stable version exists.

Installed matching errors have a different scope: `InstalledError::Missing`
means no supplied installation matched, and `Ambiguous` means several matched.
Use an installed `pack:` selector to resolve ambiguity.

| `resolution::Error` | Recovery |
| --- | --- |
| `Invalid` | Correct malformed identities, duplicate roots/releases/edges, requirements or levels |
| `Unenrolled(identity)` | Review and enroll the intended source, or remove the foreign dependency; metadata cannot enroll it |
| `Conflict` | Supply needed candidate releases and reconcile requirements or pins |
| `Cycle` | Choose releases whose reachable dependency closure is acyclic |
| `LevelConflict(instance)` | Review explicit root levels and incoming requirements |
| `Limit` | Check input counts, reachable packs and search complexity |

The resolver performs no downloading, so an incomplete candidate set may appear
as a conflict even when the registry has compatible releases. See
[Resolve dependencies](../guides/dependencies.md).

Unenrolled dependencies, cycles and level conflicts backtrack before becoming
final errors. A successful unpinned resolution may choose an older version;
`resolve_with_diagnostics` reports rejected attempts for review. Unenrolled
roots, invalid input and exhausted bounds refuse immediately. Exact pins do
not permit fallback to another version.

## Registration writes

For `RegistrationError::{InvalidAlias,Reserved,Duplicate,Identity}`, correct
the proposed alias/source rather than retrying unchanged. `Missing` or
`InvalidDefault` requires a registered alias in the appropriate scope.
`Malformed`, `Link`, `Path`, `TooLarge` and `Limit` identify invalid storage
or exceeded limits. `Io` carries a filesystem error.

The write outcomes require different responses:

| Error | New value written? | Response |
| --- | --- | --- |
| `Changed` | No | Reread, rebuild the change and review if needed |
| `Locked(path)` | No | Wait for the active writer; remove a stale lock only after confirming ownership |
| `NotDurable` | Yes | Read current state; report that crash durability was not established |
| `LockRemains(path)` | Yes | Read current state and resolve the leftover lock before another write |
| `RefusedLockRemains { .. }` | No | Handle the contained refusal and resolve the leftover lock |

Do not treat every error as a rolled-back write or retry blindly. See
[Registrations](../guides/registrations.md).

## Cache and locked state

`CacheError::Digest` means a noncanonical digest argument. `NotRegular` refuses
links and wrong file kinds. `TooLarge` is a read-limit failure; `Corrupt` means
stored bytes do not hash to their name. `Mismatch` refuses a write under the
wrong expected digest. `Io` carries the filesystem cause.

For a miss, use the locked restoration workflow. For corruption, reacquire the
verified exact bytes and repair with `put_expected`. Never substitute a newer
version under an old digest.

`LockError::{Duplicate,Malformed,MissingDependency,Unsatisfied,Unreachable,Limit}`
means the complete locked closure is invalid. Rebuild entries from a reviewed
resolution, or restore a known valid lockfile. `lock::sort` changes order only;
it does not fix missing dependencies or invalid data.

Related: [Cache artifacts](../guides/cache.md),
[Lock and reproduce installs](../guides/lockfiles.md), [Limits](limits.md).

[Documentation index](../README.md)
