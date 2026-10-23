# Trust

This topic owns what the library believes and why: how a source is
identified, how trust in a new registry begins, when a catalog is accepted,
how rollback and replacement are refused, and what remains outside the
library's protection.

## Source identity

A source is identified by what it is, so aliases, defaults and renaming never
change which source an installed pack or a dependency refers to.

| Source | Identity | Trust anchor |
| --- | --- | --- |
| Local directory | SHA-256 of the canonical absolute UTF-8 path | None; reads are confined to the directory |
| Signed HTTPS | SHA-256 of the JSON array `[location, key, registry_id]` | The Ed25519 key accepted at discovery |
| Built-in | A fixed identity chosen by the product | Location, key and registry ID compiled into the product |

Requirements:

- A signed source's location is an `https://` URL with a host, at most
  [4096 bytes](limits.md#text-and-identifiers). It may have a non-zero port
  and must end in `/`. It has no user information, query, fragment or
  percent-encoding, and its path uses only ASCII letters, digits and
  `-._~/`, with no empty, `.` or `..` segment.
- A signed source's location and key are stored, and hashed into its
  identity, in canonical form: the host in lowercase, the default port 443
  omitted, and the key as 64 lowercase hexadecimal characters. Every spelling
  of the same host, port and key is therefore one source, whether an operator
  typed it or a registry served it. The path keeps its case, because servers
  may treat it as case-sensitive. A stored source that is not in canonical
  form is refused.
- A signed source has both a key and a registry ID; a local source has
  neither and an absolute location. Identities and registry IDs are
  non-blank, [bounded](limits.md#text-and-identifiers) and free of NUL.
- A source read back from storage is validated again against these rules.
- Only a built-in source carries an identity chosen by its product. Every
  [registered](registrations.md#aliases-and-scopes) source must carry the
  identity derived from what it is, so no registration can claim another
  source's identity, including the built-in one.
- Because identity covers the key and registry ID, a registry that changes
  either is a different source. An enrolled key is never replaced by a key
  fetched from the network.

## Discovery

Discovery turns an operator-supplied location into a candidate source and its
first catalog, for the operator to review. It enrolls nothing.

- **A location beginning with `https://`:** a missing trailing `/` is added, and the result must be
  a valid signed location, which is then put in canonical form. The published
  key may surround its 64 hexadecimal characters with whitespace and use
  either case. Discovery reads `catalog-key.pub`, `catalog.json`
  and `catalog.sig` once each, verifies the signature with the published key,
  and only then reads the `registry_id` from the authenticated bytes. Those
  same bytes are then authenticated in full against the candidate source, so
  the catalog presented for review is exactly the one whose signature was
  checked.
- **Any other location:** a local directory. It is canonicalized, must be a
  directory, and must hold a valid catalog.

A product that enrolls the source after review must also admit the discovered
catalog to its [checkpoints](#checkpoints), so the first catalog seen is the
first retained.

## Built-in registry

A product's own registry is a pinned source: its location, identity, key and
registry ID are constants of the product, so it needs neither discovery nor
review. Its alias is reserved: it cannot be registered, removed or shadowed,
and it installs nothing by itself. Rotating or revoking a built-in key is a
product release.

## Catalog authentication

A catalog is accepted for a source at a given time only when:

1. the bytes are within the [document bound](limits.md#acquisition), checked
   before any other work, whether they were fetched or supplied directly;
2. for a signed source, the signature verifies with the enrolled key over the
   exact bytes, checked before the bytes are parsed;
3. the bytes are a valid [catalog](registry-format.md#catalog);
4. for a signed source, `registry_id` equals the enrolled one, a `sequence` is
   present, and `expires_at` is later than the given time. Both are compared
   as whole seconds since the Unix epoch, so the decision holds for any clock,
   including one set before 1970 or after year 9999.

An accepted catalog carries the SHA-256 of its exact bytes, which checkpoints
compare.

| Refusal (`CatalogError`) | Cause |
| --- | --- |
| `SignatureMissing` | A signed source served no signature |
| `SignatureInvalid` | The signature is malformed or does not verify |
| `Malformed` | Not a supported v1/v2 catalog: invalid JSON shape, an unknown field, an invalid ID or version, or command metadata in v1 |
| `Limit` | More bytes than the [document bound](limits.md#acquisition), or more releases or command associations than the [catalog bounds](limits.md#catalogs) |
| `Identity` | A signed catalog's `registry_id` differs from the enrolled one |
| `SequenceMissing` | A signed catalog has no `sequence` |
| `ExpiryInvalid` | `expires_at` is malformed, or missing on a signed catalog |
| `Expired` | A signed catalog's `expires_at` is not later than the given time |
| `DuplicateRelease` | A `publisher/name@version` appears twice |
| `InvalidEntry` | A release has an invalid path or digest, or command associations name a pack without a listed stable release, contain an empty list, or contain duplicate or invalid basenames |

## Checkpoints

Authentication proves who signed a catalog, not that it is current or
consistent with what came before. Checkpoints keep, per source identity, the
last admitted catalog's sequence and digest, and the digest of every release
the source has ever listed. Every authenticated catalog must be admitted
before any of its releases is used. A catalog remembers the identity of the
source it was authenticated against and is admitted under that identity only,
so one source's catalog can never update another source's checkpoint.

- A sequenced catalog below the retained sequence is a rollback.
- A catalog with the retained sequence but different bytes is also a rollback.
- A release seen before keeps its digest, even after it left the catalog;
  listing it with another digest is a replacement.
- A local source may start with unsequenced catalogs, which keep the release
  rule. Once a sequenced catalog is admitted, every later catalog must have
  a sequence. Omitting it is a rollback refusal and leaves the checkpoint
  unchanged, including its paired sequence and catalog digest.
- Admission is all-or-nothing: a refusal leaves every checkpoint unchanged.
- Sources and retained releases are [bounded](limits.md#checkpoints). A
  catalog that would add a source or release beyond a bound is refused;
  catalogs within the retained set keep being admitted. The source bound is
  above the most sources [registrations](registrations.md) can enroll at once.

| Refusal (`CheckpointError`) | Cause |
| --- | --- |
| `Rollback` | A lower sequence, a missing sequence after sequencing began, or the same sequence with different bytes |
| `Replaced(release)` | A retained release listed with another digest |
| `Limit` | A source or release bound was reached |

Checkpoints are a value the product stores. Their serialized form is a JSON
object keyed by source identity, each with `sequence` (a number, or null
for a local source that has not started sequencing), `digest`, and `releases`, an object
from `publisher/name@version` to digest. An optional `commands` object maps each
command basename to its array of pack IDs from the latest admitted catalog.
This inverted index is advisory and does not accumulate with release history.
Successful admission replaces it, including clearing it when catalog command
metadata is absent or empty. Refused admissions leave it unchanged.

A missing checkpoint `commands` field defaults to an empty index, so checkpoints
written before command discovery remain readable. An empty index is omitted
when serialized; an explicit empty object is also accepted when reading.
Older readers reject checkpoints containing `commands` as an unknown field.
Consumers must retain a compatible reader after persisting that field; clearing
checkpoint history to permit a downgrade would discard rollback and replacement
protection. Catalog v1/v2 compatibility remains a separate input contract.

Other unknown fields are refused. Stored checkpoints are not checked against
the bounds again; only admission
enforces them.

An unreachable registry or an expired catalog is not a revocation: bytes
already retained stay usable offline.

## Threat model

These are the threats to trust itself. The complete model, covering
transport, local files, resource bounds, dependency behavior and remaining
gaps, is owned by [security](security.md).

Trust protects against:

- **Tampering:** a catalog or artifact changed in transit or on the host. The
  signature covers the exact catalog bytes, and the catalog covers every
  artifact digest.
- **Rollback and replay:** an older catalog served again, or a catalog kept
  past its expiry.
- **Release substitution:** different bytes for a version already seen.
- **Key substitution:** a host replacing its key after enrollment.
- **Impersonation:** a registration claiming another source's identity,
  including the built-in one, whether it is added, read from a file or
  combined with other scopes; and one source spelled two ways to look like
  two sources.
- **Misattribution:** one source's catalog admitted to another source's
  checkpoint.

Trust does not protect against first contact, a compromised key or a
malicious release under a valid key, withholding, writable local registries,
missing revocation, or tampered local state. Each is described, with the
product's part in it, under [not protected](security.md#not-protected).

## Decisions

- **Trust on first use for custom registries.** Requiring keys to be
  distributed out of band would make adding a team registry impractical. The
  key is shown for review, bound into the identity, and never replaced.
- **Identity includes the key.** Changing a key forces a visible
  re-registration instead of silently trusting whoever controls the host.
- **Checkpoints retain releases, not just sequences.** A release's digest
  must not change even when a later catalog drops and re-adds it.
- **Sequencing cannot be implicitly reset.** A local source can opt into
  sequencing, but omitting metadata cannot erase retained protection. Adding
  a release is different from replaying a catalog that hides later releases.
- **Time is an input.** Expiry decisions take the time from the caller, so
  they are deterministic in tests and consistent across a product's
  operations.

## Gaps

- Checkpoints are never removed. A source whose registration was removed keeps
  its checkpoint, which protects it if it is registered again but counts
  toward the source bound. There is no way to forget a source's checkpoint.
