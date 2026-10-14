# Dependency resolution

Resolution selects one release for every pack reachable from the operator's
selections, or refuses. It is pure: it works only on the metadata it is given,
performs no I/O, and has no side effects.

## Inputs

- **Roots:** the packs the operator selected. Each has a source identity, a
  `publisher/name`, a version requirement, an optional exact pin, and a level.
  Roots are unique.
- **Releases:** the candidates the product acquired and validated. Each has a
  source identity, a `publisher/name`, an exact version, and the dependencies
  that release declares.
- **Dependencies:** each names a `publisher/name` and a SemVer requirement, and
  may name a source identity and a [level mapping](#levels). Without a source
  identity, a dependency comes from its parent's source, never from the
  operator's default registry. Declarations are validated before use.
- **Enrolled identities:** the source identities the operator has registered.
  A release cannot enroll a source: a root that names an unenrolled source is
  refused before any search, and a release whose dependencies name one
  cannot be selected.

## Selection

- **Highest compatible first.** Candidates are tried from the highest version
  down, packs in source and `publisher/name` order, backtracking whenever a
  choice leaves no valid closure. The result does not depend on input order.
- **Failures backtrack.** A candidate whose closure names an unenrolled
  source, requires a stronger level for a root, or forms a cycle is passed
  over like a version conflict, and the next lower candidate is tried.
  Resolution refuses only when no candidate works, and then reports the
  failure of the last candidate tried, or `Conflict` when there was none.
  `Invalid` and `Limit` end the search at once.
- **Reachable only.** Only packs reachable from the roots are selected, and
  only their sources must be enrolled. A cycle among unreachable releases has
  no effect. Every supplied release must still be valid.
- **Pins.** A pinned root selects exactly its pinned version, or resolution
  fails.
- **Prereleases.** A prerelease matches a requirement only when the requirement
  is one exact comparator naming it, such as `=1.2.0-beta.1`. A pinned root's
  own requirement uses ordinary SemVer matching, and when it has no
  prerelease comparator it is also satisfied by the pin's numeric version, so
  a root pinned to `1.2.0-beta.1` satisfies `^1.2`. Dependencies on it
  still need the exact form.

Each selected release is reported with its effective level, and whether it
was selected directly (a root) or as a dependency, in source and
`publisher/name` order.

## Review diagnostics

`resolve` returns selections without collecting diagnostic history.
`resolve_with_diagnostics` returns a `ResolutionReport` with the same selections
and rejected candidate attempts. Both use the same search, input validation and
budget, and return the same error if resolution fails. Neither performs review
or I/O.

Each `RejectedCandidate` borrows its supplied release and records the error
returned by that attempted branch. Records are in deterministic completion
order, including failures from nested searches, and their count is bounded by
the search-state budget. Candidates excluded by requirements or pins are not
attempted or recorded. No diagnostic report is returned on failure.

A rejection describes a branch under the choices being tried, not an assertion
that the release is always unusable. The same release can appear more than once
and can be selected under other choices. Products present selected versions and
relevant rejected attempts for review, and compare with installed versions when
an update would downgrade a pack.

## Levels

A product may give each installed pack an ordered level. The product lists
every level once, strictly weakest first; resolution refuses as invalid a
level list that is empty or out of order, and any level it does not list.
Products without levels have a single level, so levels have no effect.

- A dependency gets its parent's level by default. A level mapping instead
  gives one required level for each parent level, in order. A mapping covers
  every level and never decreases.
- A pack depended on by several parents gets the strongest level any of them
  requires, decided after every parent has contributed.
- A root keeps its explicit level. A release whose closure requires a
  stronger level for a root cannot be selected.

## Refusals

Inputs and search work are [bounded](limits.md#resolution). No refusal returns
a partial selection.

| Refusal (`resolution::Error`) | Cause |
| --- | --- |
| `Invalid` | A blank, oversized or NUL-containing source identity (of a root, release, dependency or enrolled source); a dependency pack that is not `publisher/name`; an unparsable requirement, or one over its comparator or text [bound](limits.md#text-and-identifiers); an oversized version; an unlisted level or malformed level list; a mapping that is incomplete or decreases; or a duplicate root, release (same pack and version) or dependency (same source and pack within one release) |
| `Limit` | Too many roots, releases, dependencies, enrolled sources or reachable packs, or the search budget ran out |
| `Unenrolled(identity)` | A root names an unenrolled source, or the last candidate tried depended on one |
| `Conflict` | No set of releases satisfies every requirement and pin |
| `LevelConflict(instance)` | The last candidate tried required a stronger level for a root than its explicit one |
| `Cycle` | The last candidate tried formed a dependency cycle |

## Decisions

- **Deterministic search.** The same inputs always produce the same selection,
  so a review shows exactly what will be installed.
- **Backtracking preserves valid installations.** An unusable newer release
  must not block an older release with a valid closure. Exact pins refuse
  instead of changing the requested version. Optional diagnostics make these
  candidate failures available to products without changing selection.
- **Dependencies inherit their parent's source.** A pack cannot redirect its
  dependencies to the operator's default registry, which may list a
  different pack under the same name.
- **Levels are generic.** The library orders and propagates levels without
  knowing their meaning, so products with different settings, or none, share
  one resolver.
