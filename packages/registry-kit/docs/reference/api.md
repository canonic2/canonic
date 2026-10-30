# API map

The guides show workflows. Use generated rustdoc for exact signatures, trait
bounds and every typed error variant. From the library directory, build the
public API reference:

```sh
cargo doc --locked --no-deps --open
```

Add `--features test-support` to include consumer test fixtures. This package is
unpublished, so its API reference is built locally rather than hosted on docs.rs.

## Choose an entry point

| Task | API | Guide |
| --- | --- | --- |
| Discover a new source | `acquisition::Fetcher::discover`, `Discovery` | [Sources and trust](../guides/sources-and-trust.md) |
| Construct an already trusted source | `source::Source::{local,signed,pinned}` | [Sources and trust](../guides/sources-and-trust.md) |
| Fetch catalogs and releases | `Fetcher::{catalog,artifact,read}` | [Sources and trust](../guides/sources-and-trust.md) |
| Verify supplied catalog bytes | `catalog::Catalog::verify` | [Sources and trust](../guides/sources-and-trust.md) |
| Retain rollback history | `checkpoint::Checkpoints::{admit,get,release_digest}` | [Sources and trust](../guides/sources-and-trust.md) |
| Manage aliases and defaults | `registrations::{Registrations,Effective,Scope}` | [Registrations](../guides/registrations.md) |
| Read and replace global state | `registrations::GlobalFile` | [Registrations](../guides/registrations.md) |
| Parse user input | `selector::{parse,Selector,Reference}` | [Selecting packs](../guides/selecting-packs.md) |
| Browse releases | `Catalog::{lookup,select,release,releases,search,entries}` | [Selecting packs](../guides/selecting-packs.md) |
| Identify an installation | `selector::{InstalledSelector,Installed,find_installed}` | [Selecting packs](../guides/selecting-packs.md) |
| Resolve versions and levels | `resolution::{resolve,Root,Release,Dependency,Instance,Selection,Level}` | [Dependencies](../guides/dependencies.md) |
| Explain candidate fallback | `resolution::{resolve_with_diagnostics,ResolutionReport,RejectedCandidate}` | [Dependencies](../guides/dependencies.md#explain-fallback-during-review) |
| Store and read retained bytes | `cache::ArtifactCache::{put,put_expected,get,contains}` | [Cache](../guides/cache.md) |
| Record and check a closure | `lock::{LockedPack,LockedEdge,sort,validate,incoming}` | [Lockfiles](../guides/lockfiles.md) |
| Serve signed fixtures | `testing::{SignedRegistry,Server,Response,SigningKey}` | [Testing](../guides/testing.md) |

## Text, digest and time helpers

`names::PackId` implements `FromStr`, `Display` and Serde. Build it with
`PackId::new(publisher, name)` or parse `"acme/git"`; access segments with
`publisher()` and `name()`. `names::{is_identifier,is_source_identity,
is_artifact_path}` expose the input rules.

`digest::sha256_hex` hashes bytes. `encode` emits lowercase hex. `decode`
accepts fixed-length hex with surrounding whitespace, while `normalize_hex`
and `normalize_sha256` accept either case without surrounding whitespace.
`is_sha256_hex` checks canonical lowercase digest text. See
[Catalog format](../publishing/catalog-format.md) for where each form is used.

`timestamp::parse` validates UTC calendar text and returns calendar parts; it
accepts optional fractional seconds. Catalog expiry is stricter:
`is_utc_seconds` requires whole seconds and `is_expired` compares that expiry
with a caller-supplied `SystemTime`, returning `None` for invalid text.
`unix_seconds` converts validated calendar parts to epoch seconds;
`format_utc` formats epoch seconds for publication metadata. Validate the
formatted result with `is_utc_seconds` before using it as catalog expiry.

## Persistence and errors

`Source`, `Registrations`, `Checkpoints`, `Checkpoint`, `Entry`, `LockedPack`
and `LockedEdge` support Serde. `PackId` and `InstalledSelector` serialize as
strings. `Catalog` is obtained through verification, not deserialization.
Resolution structs are in-memory inputs and results; your application owns any
stored representation of requirements or levels.

Each fallible module exposes typed errors. Branch on variants when handling
specific outcomes and display context such as source, pack and operation in your
application. See [Errors and troubleshooting](errors.md).

[Documentation index](../README.md)
