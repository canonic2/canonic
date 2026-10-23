# Registry format

A registry is defined by what it serves. This topic owns that format and the
rules a publisher must follow. How the library checks a registry is owned by
[trust](trust.md) and [acquisition](acquisition.md).

## Files

A registry is a directory, read in place when local, or served under an HTTPS
base URL ending in `/`:

| File | Present | Content |
| --- | --- | --- |
| `catalog.json` | always | The catalog |
| `catalog.sig` | signed registries | Detached signature over `catalog.json` |
| `catalog-key.pub` | signed registries | The signing public key |
| artifact paths | always | One file per listed release |

Local registries are unsigned. HTTPS registries must be signed: an unsigned
HTTPS registry cannot be enrolled.

## Catalog

`catalog.json` is a JSON object with version 1 or 2 of this schema. Unknown fields
are refused at every level.

| Field | Required | Meaning |
| --- | --- | --- |
| `schema_version` | always | `1` or `2` |
| `packs` | always | The listed releases, within the [catalog bound](limits.md#catalogs) |
| `registry_id` | signed | The registry's stable ID: non-blank, at most [4096 bytes](limits.md#text-and-identifiers), without NUL. A signed catalog's must equal the enrolled one; a local catalog's is not checked |
| `sequence` | signed | Non-negative publication counter |
| `expires_at` | signed | Whole-second UTC time, `YYYY-MM-DDTHH:MM:SSZ`; the catalog is invalid from that second on |
| `commands` | optional, v2 only | Map of listed `publisher/name` IDs to command basenames; advisory relevance, never permission |

Each `commands` key must have a stable release in the same catalog. Each list
contains 1–16 unique basenames, each 1–64 ASCII characters, starting with a letter
or digit and continuing with letters, digits or `._+-`. Paths and shell syntax
are refused. The map contains at most 64 packs. Associations are catalog-owner
metadata authenticated by the same exact-byte signature, independent of artifact
contents and immutable release digests. They may change at a higher sequence.

Version 1 has no command metadata; consumers must not infer relevance from pack
names. Version 2 preserves the release schema. Clients that support only v1 refuse
v2; publishers must coordinate that upgrade before serving it. Pack artifact
schemas are owned by consumers and are unchanged by this catalog version.

Checkpoint admission prepares an inverted command-to-pack index from the latest
catalog only. It replaces that index on each successful admission, including
clearing it when metadata is absent. Release-digest retention remains separate.
Consumers can use the bounded index without fetching artifacts, compiling policy
or scanning retained releases. Consumers own executable-path normalization,
current source enrollment, installed-instance exclusion and advisory presentation.

A present `expires_at` must be well formed on any registry. Only signed
registries must have one, and only for them is it enforced.

Each release in `packs` has:

| Field | Rule |
| --- | --- |
| `id` | `publisher/name`; each segment 1–64 lowercase ASCII letters, digits or hyphens |
| `version` | Exact SemVer, including prerelease and build metadata; versions that differ only in build metadata are different releases |
| `path` | Safe, portable relative path (below) |
| `sha256` | Exactly 64 hexadecimal characters, either case, with no whitespace: the SHA-256 of the artifact |

An artifact path:

- uses only ASCII letters, digits and `-._~/`;
- has no empty, `.` or `..` segment, and no segment ending in `.`;
- has no Windows device name segment (`CON`, `PRN`, `AUX`, `NUL`,
  `COM1`–`COM9`, `LPT1`–`LPT9`), with or without an extension.

These rules keep every path inside the registry and storable on every
platform.

A `publisher/name` and `version` pair appears at most once. One invalid release
makes the whole catalog invalid, so a catalog is either fully usable or not
used at all.

## Signing

`catalog-key.pub` holds a 32-byte Ed25519 public key as 64 hexadecimal
characters. `catalog.sig` holds a 64-byte Ed25519 signature over the exact
bytes of `catalog.json`, as 128 hexadecimal characters. Whitespace around
either value is ignored. Because the signature covers bytes rather than parsed
JSON, `catalog.json` must be served exactly as it was signed.

## Publisher rules

Enrolled clients enforce these rules, so a registry that breaks them stops
working for those clients:

- **Releases are immutable.** A `publisher/name@version` listed once keeps its
  digest forever, even after it is removed from the catalog.
- **The sequence grows.** Every change to `catalog.json` needs a higher
  `sequence`. Identical bytes may be republished with the same sequence.
  Local registries may start without sequencing, but once a sequenced catalog
  has been admitted by a client, that client refuses an unsequenced catalog.
- **Catalogs are renewed before they expire.** From `expires_at` on, clients
  refuse the catalog until a newer one is published.
- **Identity is stable.** A signed registry is identified by its location, key
  and `registry_id`; changing any of them makes it a different registry that
  every client must register again.
- **Responses are plain.** HTTP 200, no redirects, no `Content-Encoding`, within
  the [response bounds](limits.md#acquisition).

## Decisions

- **One catalog file per registry.** A registry is a static directory, so it
  can be hosted on any static file host and mirrored by copying it. The
  [catalog bound](limits.md#catalogs) keeps the file small.
- **Signatures over exact bytes.** Verifying bytes before parsing means a
  parser is never exposed to unauthenticated input from a signed registry,
  and needs no canonical JSON form.
- **Whole-catalog validity.** Accepting part of a catalog would let one
  malformed release hide others' problems and make checkpoints partial.
