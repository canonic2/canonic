# Core concepts

## Packs and releases

A **pack** has a fully qualified name such as `acme/git`. A **release** is that
pack at an exact SemVer version, such as `1.2.0`. Its catalog entry records a
relative artifact path and the SHA-256 of the artifact's exact bytes.

Registry treats artifacts as bytes. JSON, YAML, archives and their dependency
declarations are application choices. Validate a pack before converting its
declarations into resolver metadata or using its contents.

## Sources and aliases

A **source** is a canonical local directory or a signed HTTPS base URL. A
**registration** assigns a source an alias, such as `team`, within a global or
project scope. Aliases make references convenient; they do not identify
installed packs.

| Name | Example | Used for |
| --- | --- | --- |
| Alias | `team` | Selecting a registered source |
| Pack ID | `acme/git` | Identifying a pack within a source |
| Registry ID | `acme-team` | Checking the signed catalog's declared identity |
| Source identity | SHA-256 text, or a built-in fixed identity | Dependencies, checkpoints and locked entries |
| Installed selector | `pack:` followed by a UUID-shaped value | Addressing one installed instance |

A signed source's derived identity binds its canonical URL, signing key and
registry ID. A local source's identity binds its canonical directory path.
Changing an alias leaves that identity unchanged. Moving a registry or changing
its signing key creates a different derived identity.

## Verification and admission

Catalog verification checks format and, for HTTPS sources, the enrolled signing
key, registry ID and expiry. **Checkpoint admission** then compares the catalog
with previously accepted catalogs to refuse rollback and changed release bytes.
Call both before using a fetched catalog.

Discovery checks a new source and returns it for review. It does not register it
or establish the operator's approval. The application decides whether to enroll
the returned source and stores that decision.

## Retained state

| State | Purpose | Who writes it |
| --- | --- | --- |
| Registrations | Sources and default aliases | `GlobalFile` for global scope; your application for project scope |
| Checkpoints | Accepted catalog sequence and historical release digests | Your application serializes `Checkpoints` |
| Artifact cache | Exact release bytes addressed by digest | `ArtifactCache` at your chosen path |
| Lockfile | Selected releases and dependency edges | Your application embeds `LockedPack` entries |

Retain checkpoints across runs. A cache verifies bytes against a digest, but it
does not establish that a publisher or a pack is acceptable. A lockfile describes
an installation; the application still validates and activates its contents.

Next: [Integration workflow](../guides/integration.md).

[Documentation index](../README.md)
