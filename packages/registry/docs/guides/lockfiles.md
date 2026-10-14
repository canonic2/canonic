# Lock and reproduce installs

Embed `LockedPack` entries in your application's lockfile document. Registry
defines and validates the entries; your application defines the outer schema,
file path, document size bound, writes and activation.

## Construct entries

Each selected release records these fields:

| Field | Value |
| --- | --- |
| `selector` | Stable installed instance selector |
| `registry` | Captured source identity |
| `id`, `version` | Fully qualified pack ID and exact release version |
| `digest` | Lowercase SHA-256 from the corresponding catalog entry |
| `pinned` | Whether the application retains an exact request during updates |
| `direct` | Whether this is a root selection |
| `dependencies` | This release's edges with explicit source identities |

The following helper builds an entry from a resolver selection and its matched
catalog entry. The application must verify that both came from the same source
and release; this helper performs no authentication.

```rust
use canonic_registry::{
    catalog::Entry, lock::{LockedEdge, LockedPack},
    resolution::{Level, Selection}, selector::InstalledSelector,
};

fn locked<L: Level>(
    selection: &Selection<'_, L>, entry: &Entry,
    selector: InstalledSelector, pinned: bool,
) -> LockedPack {
    let release = selection.release;
    LockedPack {
        selector,
        registry: release.instance.registry.clone(),
        id: release.instance.pack.clone(),
        version: release.version.clone(),
        digest: entry.sha256.clone(),
        pinned,
        direct: selection.direct,
        dependencies: release.dependencies.iter().map(|edge| LockedEdge {
            registry: edge.registry.clone()
                .unwrap_or_else(|| release.instance.registry.clone()),
            pack: edge.pack.clone(),
            requirement: edge.requirement.clone(),
        }).collect(),
    }
}
```

Give a new installation a unique `InstalledSelector` seed. Preserve the selector
when updating the same instance. Calculate `pinned` from your root request or
existing lock state; the resolver's `Selection` does not contain this flag.
Levels belong in your own state because locked entries do not store them.

Call `lock::sort(&mut packs)` for source-identity/pack-ID order, then
`lock::validate(&packs)` before writing. For example, a product-defined document
can serialize `serde_json::json!({"schema_version": 1, "packs": packs})`.
That outer `schema_version` is your schema, not a Registry lockfile format.

## Validate on load

Deserialize entries into `Vec<LockedPack>`, then call `lock::validate`.
Deserialization checks typed fields and refuses unknown entry/edge fields;
it does not replace closure validation.

Validation checks at most 64 entries, unique selectors and source/pack pairs,
well-formed identities and digests, bounded requirements, satisfied dependency
edges and reachability from a direct selection. It does not authenticate a
source, verify cached bytes or prove that edges match artifact contents. It
also does not provide the resolver's cycle check; use validated resolver output
when creating installations.

Incoming edges are derived, not separately persisted:
`lock::incoming(&packs, identity, &id)` returns each dependent pack and its edge.
Use this to explain constraints before an update or removal. Removal and
re-resolution are application workflows; deleting an entry may invalidate the
remaining closure.

## Restore an installation

1. Load and validate the locked entries.
2. Read each artifact with `cache.get(&pack.digest, your_limit)`.
3. If bytes are present, validate the application pack format and use them
   through your installation workflow. No catalog or network access is needed.
4. For a miss, find the enrolled source by `pack.registry`, not an alias or the
   current default. `Effective::by_identity` can find an available registration.
5. Fetch and admit that source's catalog. Find `catalog.release(&pack.id,
   &pack.version)` and require `entry.sha256 == pack.digest` before acquisition.
6. Fetch with `artifact`, retain with `put_expected`, then validate and activate.

If the source is unavailable, the release was removed or its digest differs,
report that exact locked release cannot be restored. Choosing another version
would require a separate update and review. A removed registration does not
prevent offline cache reads, but it can prevent recovering missing artifacts.

## Update an installation

Convert direct entries back to roots using your retained requirements and
levels. Set `Root::pin` to an entry's exact version when `pinned` is true.
Registry does not implement an update command or enforce the lock flag by itself.
Acquire candidate metadata, resolve, review, cache and write a new complete
lockfile. Keep unchanged instance selectors stable.

Related: [Resolve dependencies](dependencies.md), [Cache artifacts](cache.md).

[Documentation index](../README.md)
