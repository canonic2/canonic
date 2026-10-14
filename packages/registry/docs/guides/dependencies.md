# Resolve dependencies

`resolution::resolve` chooses one release for every source-qualified pack
reachable from your roots. Supply authenticated, validated metadata; the resolver
performs no network I/O, content validation, enrollment or installation.

## Describe candidates and roots

This standalone example selects `acme/app@1.0.0` and its compatible dependency:

```rust
use canonic_registry::resolution::{self, Dependency, Instance, Release, Root};
use semver::VersionReq;
use std::{collections::BTreeSet, error::Error};

fn resolve_example() -> Result<(), Box<dyn Error>> {
    let source = "acme-enrolled-source"; // stand-in for a captured Source::identity()
    let app = Instance { registry: source.into(), pack: "acme/app".parse()? };
    let tokens = Instance { registry: source.into(), pack: "acme/tokens".parse()? };
    let releases: Vec<Release<()>> = vec![
        Release {
            instance: app.clone(), version: "1.0.0".parse()?,
            dependencies: vec![Dependency::parse("acme/tokens", "^1", None, None)?],
        },
        Release {
            instance: tokens.clone(), version: "1.4.0".parse()?, dependencies: vec![],
        },
        Release {
            instance: tokens, version: "2.0.0".parse()?, dependencies: vec![],
        },
    ];
    let roots = vec![Root {
        instance: app, requirement: VersionReq::STAR, pin: None, level: (),
    }];
    let enrolled = BTreeSet::from([source.to_owned()]);
    let selected = resolution::resolve(&roots, &releases, &enrolled)?;
    assert_eq!(selected.len(), 2);
    assert!(selected.iter().any(|s|
        s.release.instance.pack.to_string() == "acme/tokens"
        && s.release.version.to_string() == "1.4.0" && !s.direct
    ));
    Ok(())
}
```

In production, capture identities from approved sources and use
`Effective::identities()` for `enrolled`. The stand-in above demonstrates the
pure API; a string in release metadata is not evidence of enrollment.

`Dependency::parse(pack, requirement, registry, levels)` validates each edge.
An omitted registry means the parent's source, never the user's default.
An explicit registry is a source identity, not an alias or URL. A reachable
unenrolled source makes that candidate unusable even if you supply its releases.
A root whose own source is unenrolled is refused before searching.

## Version selection and pins

Candidates are tried highest first in source/pack order. The search backtracks
when a candidate has no compatible closure. Input order does not affect the
selection. Only reachable candidates are selected, but every supplied release
must be valid. Supply older candidates when they may satisfy a closure.

An unenrolled dependency, cycle or root-level conflict rejects that candidate
and allows a lower version to be tried. For example, if `2.0.0` needs an
unenrolled source but `1.8.0` has a valid closure, an unpinned request can select
`1.8.0`. An exact pin to `2.0.0` refuses instead of changing versions.

`Root::requirement` constrains allowed versions. Set `pin` for an exact request
or an existing pinned installation. A pin must have a supplied compatible
release. It is not an instruction to fetch that release.

Prerelease dependencies require one exact comparator, such as
`=1.2.0-beta.1`; a broad range does not opt into prereleases. A root pinned to a
prerelease can satisfy a prerelease-free root range by its numeric version.
Incoming dependency requirements still need the exact prerelease form.

Each returned `Selection` borrows a supplied `Release` and records its level
and `direct` status. A direct entry is a root; the others are dependencies.
Keep candidates alive while using the results. Selected cycles are refused.

## Explain fallback during review

Use `resolve_with_diagnostics` when presenting a resolution to the operator:

```rust
use canonic_registry::resolution::{self, Release, Root};
use std::collections::BTreeSet;

fn review_candidates(
    roots: &[Root<()>], releases: &[Release<()>], enrolled: &BTreeSet<String>,
) -> Result<(), resolution::Error> {
    let report = resolution::resolve_with_diagnostics(roots, releases, enrolled)?;
    for selected in &report.selections {
        println!("Selected {}@{}", selected.release.instance.pack, selected.release.version);
    }
    for attempt in &report.rejected {
        println!("Could not complete a closure with {}@{}: {}",
            attempt.release.instance.pack, attempt.release.version, attempt.reason);
    }
    Ok(())
}
```

`ResolutionReport::selections` is the same result as `resolve`. `rejected`
records attempted candidates and their branch failures in deterministic
completion order, within the existing search budget. It does not include
candidates excluded by requirements or pins. On failure, the method returns
the same typed error without a partial report.

An attempt can fail under one set of choices and succeed under another; a
release can occur more than once in diagnostics or also appear in the selected
closure. Treat the records as search context, not a global rejection list.
Present the chosen versions before installation. During updates, compare them
with installed versions and call out any downgrade for review. Registry supplies
the report; the application's UI and approval workflow decide how to show it.

## Selection levels

Use `()` when all packs have one level. If your application has ordered levels,
implement `Level` and list every value strictly weakest first:

```rust
use canonic_registry::resolution::{Dependency, Level};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Profile { Basic, Strict }

impl Level for Profile {
    fn all() -> &'static [Self] { &[Self::Basic, Self::Strict] }
}

fn strict_dependency() -> Result<Dependency<Profile>, canonic_registry::resolution::Error> {
    Dependency::parse("acme/review", "^1", None,
        Some(vec![Profile::Strict, Profile::Strict]))
}
```

Without a mapping, a dependency inherits its parent's level. With a mapping,
each position gives the required level for the corresponding parent level.
The mapping must cover every level and must not decrease as the parent grows
stronger. Multiple parents contribute the strongest required level.

A root keeps its explicit level. If a dependency requires it to be stronger,
resolution returns `LevelConflict`; it does not silently change the root.

## Refusals

`Invalid` covers malformed or duplicate metadata and invalid level mappings.
`Unenrolled`, `Conflict`, `Cycle` and `LevelConflict` identify unavailable
sources or incompatible closures. `Limit` includes both oversized inputs and
the deterministic search budget. No error returns a partial closure. See
[Limits](../reference/limits.md) and [Errors](../reference/errors.md).

Next: [Lock and reproduce installs](lockfiles.md).

[Documentation index](../README.md)
