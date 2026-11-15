# Find and select packs

Parse a user's catalog reference with `selector::parse`. Supply the current
registered aliases so two-segment references can be interpreted correctly.

| Reference | Meaning |
| --- | --- |
| `git` | Bare pack name in the default source |
| `team/git` | Bare name in alias `team`, if registered |
| `acme/git` | Publisher-qualified name in the default source, if `acme` is not an alias |
| `team/acme/git` | Publisher-qualified name in alias `team` |
| `team/acme/git@1.2.0` | Exact release, recorded as a pin |
| `pack:xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx` | One installed instance |

A registered alias wins over a publisher in a two-segment reference. Use three
segments to disambiguate. Version suffixes are exact SemVer versions, including
prereleases; `@^1` is invalid. Inputs are bounded to 4096 bytes.

## Acquire a selected release

This helper works with existing effective registrations and checkpoints:

```rust
use canonic_registry_kit::{
    acquisition::Fetcher, checkpoint::Checkpoints, registrations::Effective,
    selector::{self, Selector},
};
use std::{error::Error, time::SystemTime};

fn acquire(
    input: &str, effective: &Effective, fetcher: &Fetcher,
    checkpoints: &mut Checkpoints, now: SystemTime,
) -> Result<Vec<u8>, Box<dyn Error>> {
    let Selector::Catalog(reference) =
        selector::parse(input, |alias| effective.get(alias).is_some())?
    else {
        return Err("use installed state for a pack: selector".into());
    };
    let source = effective.select(reference.alias.as_deref())?;
    let catalog = fetcher.catalog(source, now)?;
    checkpoints.admit(&catalog)?;
    // Persist admission in the application's state before relying on it later.
    let id = catalog.lookup(&reference.pack)?;
    let entry = catalog.select(&id, reference.version.as_ref())
        .ok_or("requested release unavailable")?;
    Ok(fetcher.artifact(source, entry, 64 * 1024)?)
}
```

This obtains bytes, not an installed pack. Validate contents and resolve
dependencies before activation. Preserve `reference.version` as the root pin
when resolving an exact request.

## Browse a catalog

`lookup("acme/git")` requires that fully qualified pack. `lookup("git")`
requires exactly one publisher with that bare name. `LookupError::Ambiguous`
contains the matching `PackId` values so your UI can offer explicit choices.

`select(&id, None)` returns the highest stable release. A pack containing only
prereleases yields `None`. `select(&id, Some(&version))` selects that exact
release, including prereleases. `release` also performs exact lookup.

Use `entries_page`, `heads_page`, `search_page` and `releases_page` for listings.
All take a validated `catalog::PageLimit` (default 50, maximum 100); zero and
values above 100 return `InvalidPageLimit`, rather than being clamped.
`Page.items` borrows the entries and `Page.next_position` is absent at the end.
Pages also stop before the compact JSON items array would exceed 256 KiB,
including array delimiters and separators. A single entry larger than that is
returned alone so traversal advances. Products enforce their transport limits
and any additional response fields when encoding these borrowed entries.

```rust
use canonic_registry_kit::catalog::{Catalog, EntryPosition, Page, PageLimit};

fn search<'a>(
    catalog: &'a Catalog, query: &str, after: Option<&EntryPosition>,
) -> Page<'a, EntryPosition> {
    catalog.search_page(query, PageLimit::default(), after)
}
```

Pass `next_position.as_ref()` to the next call with the same query. Products
encode these typed positions into their own cursors, binding the source,
operation and query. Registry Kit has no MCP dependency or cursor encoding.

`entries_page` includes loaded history; `heads_page` includes only discovery
releases. Both order by publisher, name, then ascending version. `search_page`
performs a case-sensitive substring match on `publisher/name` and returns each
matching release in that same order. Every match has equal relevance; an empty
query matches all. `EntryPosition { id, version }` holds the last returned key.

`releases_page(&id, limit, after)` lists one pack's loaded releases in descending
version order, with `Option<&semver::Version>` as its position. Version ordering
uses `semver::Version::Ord`, including prereleases and build metadata to break
ties. All page positions are exclusive: deleting the boundary entry or inserting
entries before it does not shift the continuation. New entries before the key
are visible on a fresh listing. Paging never fetches history; use
`Fetcher::history` first when older versions are needed.

The whole `entries` and `heads` slices remain public for integrity checks,
checkpoint comparisons and dependency workflows. `releases` remains public for
resolution and exact-pin recovery and can allocate up to 4,096 references.
`search` returns every match in catalog order; use `search_page` for listings.

## Address installed instances

Create an `InstalledSelector` once with `from_seed`, using unique installation
seed bytes chosen by your application. Include enough context to distinguish
installations, and persist the result. Reuse it while the same instance remains
installed, including version updates. It is a digest-derived UUID-shaped value,
not a random UUID generator.

```rust
use canonic_registry_kit::{names::PackId, selector::{self, Installed, InstalledSelector}};
use std::error::Error;

fn match_example() -> Result<(), Box<dyn Error>> {
    let id: PackId = "acme/git".parse()?;
    let selector = InstalledSelector::from_seed(b"acme-project:team:git:install-1");
    let index = selector::find_installed([
        Installed { selector: selector.as_str(), alias: Some("team"), id: &id },
    ], "team/git@1.0.0")?;
    assert_eq!(index, 0);
    Ok(())
}
```

`find_installed` returns the matched position in the supplied iteration. It
ignores a version suffix. When the first segment is an alias of any installed
entry, matching is scoped to that alias. Otherwise it accepts a selector,
qualified name, bare name or alias-prefixed name. Zero matches returns
`Missing`; several returns `Ambiguous`. Ask for the stable `pack:` selector to
disambiguate. Installed matching uses installed aliases supplied by the caller,
not the current default source.

Related: [Resolve dependencies](dependencies.md), [Lockfiles](lockfiles.md).

[Documentation index](../README.md)
