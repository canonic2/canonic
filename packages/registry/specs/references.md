# References

This topic owns how what an operator writes becomes a pack: reference syntax,
finding packs and releases in a catalog, and finding installed packs.

## Reference syntax

A reference is at most [4096 bytes](limits.md#text-and-identifiers):

| Form | Meaning |
| --- | --- |
| `pack:xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx` | An installed instance, by its selector |
| `name` | Pack `name` in the default registry |
| `alias/name` | Pack `name` in the registry registered as `alias` |
| `publisher/name` | Pack `publisher/name` in the default registry |
| `alias/publisher/name` | Pack `publisher/name` in the registry registered as `alias` |

- Every segment is an identifier: 1–64 lowercase letters, digits or hyphens.
- In a two-segment reference, the first segment is an alias when that alias is
  registered, and a publisher otherwise. A registered alias wins over a
  publisher of the same name; three segments are always unambiguous.
- An optional `@version` suffix is an exact SemVer version, prereleases and
  build metadata included, and pins the selection.

A reference names an alias, or none for the default registry, and a pack.
[Effective registrations](registrations.md#effective-registrations) turn the
alias into a source.

| Refusal (`SelectorError`) | Cause |
| --- | --- |
| `Limit` | The reference is too long |
| `Version` | The `@` suffix is not an exact version |
| `Reference` | Not one to three valid segments |
| `Installed` | A `pack:` reference that is not a valid selector |

## Catalog lookups

In an [authenticated](trust.md#catalog-authentication) catalog:

- A `publisher/name` names exactly that pack. A bare name names the one
  publisher's pack that has it. A missing pack is reported as missing; a bare
  name listed by several publishers is reported as ambiguous, with every
  `publisher/name` that matches.
- Without a version, a pack's selected release is its highest stable
  (non-prerelease) version; a pack with only prereleases has none. With a
  version, only that exact release is selected.
- A pack's releases can be listed highest first, and releases can be searched
  by a substring of `publisher/name`, returning every matching release in
  catalog order.

| Refusal (`LookupError`) | Cause |
| --- | --- |
| `Missing` | No listed pack matches |
| `Ambiguous(ids)` | A bare name is listed by several publishers; `ids` holds every match, in order |

## Installed selectors

Each installed instance has a selector, `pack:` followed by a UUID-formatted
lowercase hexadecimal value, that never changes while it is installed.
Selectors are derived from the SHA-256 of seed bytes the product chooses. The
product includes enough context (such as project, source, pack and time) that
two installations never share a seed.

## Installed matching

A reference to something already installed is matched against the installed
instances. Each instance is described by its selector, its `publisher/name`,
and the alias the product associates with its source. Any `@version` suffix is
ignored.

- When the reference's first segment is the alias of some installed instance,
  only that alias's instances are considered, matched by `publisher/name` or
  bare name.
- Otherwise an instance matches by its selector, `publisher/name`, bare name,
  `alias/publisher/name` or `alias/name`.
- Exactly one instance must match.

| Refusal (`InstalledError`) | Cause |
| --- | --- |
| `Missing` | No installed instance matches |
| `Ambiguous` | Several instances match; the operator must use the `pack:` selector |

## Decisions

- **Short references for the common case.** Operators write `git` or
  `team/git`; the longer forms exist to disambiguate.
- **Stable selectors.** Names can be ambiguous across registries; a selector
  identifies one installed instance for its whole life.
