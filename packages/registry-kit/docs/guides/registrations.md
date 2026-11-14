# Registrations

Use `Registrations` for one scope's aliases and default. Use `Effective` to merge
the built-in source, user-global scope and optional project scope.

## Merge scopes

```rust
use canonic_registry_kit::{
    registrations::{Effective, Registrations}, source::Source,
};
use std::error::Error;

fn with_team(builtin: &Source, team: Source) -> Result<Effective, Box<dyn Error>> {
    let mut global = Registrations::default();
    global.add("team", team, "main")?;
    let available = Effective::merge(("main", builtin), &global, None)?;
    global.set_default("team", &available)?;
    Ok(Effective::merge(("main", builtin), &global, None)?)
}
```

The example assumes `team` is an approved source with a derived identity. Supply
one valid reserved alias, such as `main`, consistently to all registration calls.
Aliases use 1–64 lowercase ASCII letters, digits or hyphens.

Project aliases shadow global aliases of the same name. Neither scope can
register or remove the built-in alias. Each scope permits at most 64 aliases.
Several aliases may name the same source.

The default is the project's explicit default, otherwise the global default,
otherwise the built-in alias. `select(Some("team"))` selects an explicit alias;
`select(None)` selects the default. A project default may refer to a source from
another scope. If it becomes unavailable, `select` reports `Missing`; it does
not silently fall back.

To set a global default, pass an `Effective` merged without project scope to
`set_default`. Clear `Registrations::default` to `None` to inherit. `remove`
clears the scope's default when it named the removed alias. It does not uninstall
packs or erase checkpoints.

`iter` reports aliases and their scopes in alias order. `by_identity` finds the
first matching alias in that order. `identities` produces the enrolled source
set for dependency resolution. Capture identities before recording installed
state; do not retain aliases as dependency identities.

## Persist global registrations

`GlobalFile` reads committed registrations and publishes reviewed generations at
your chosen path. Its JSON file is a readable projection; before the first
generation, a missing file reads as empty registrations.

```rust
use canonic_registry_kit::{registrations::GlobalFile, source::Source};
use std::{error::Error, path::Path};

fn store_approved(path: &Path, source: Source) -> Result<(), Box<dyn Error>> {
    let file = GlobalFile::new(path, "main");
    let before = file.read()?;
    let mut after = before.clone();
    after.add("team", source, "main")?;
    // Review this exact before/after change in the application before writing.
    file.replace(&before, &after)?;
    Ok(())
}
```

Replacement validates `after`, takes a kernel lease and publishes only if the
captured base remains current. On `Changed`, reread and rebuild the proposed
change; obtain review again if what was approved has changed.

For a frozen review and recoverable application receipt, use `snapshot`, then
`publish(&snapshot, &after, operation_digest, &receipt)`. The snapshot also binds
the generation: an intervening change and reversal still make it stale. Supply a
64-character lowercase SHA-256 operation key and a JSON receipt no larger than
64 KiB. Your application owns review and the receipt's meaning. Call `recover`
before a later mutation to reconcile interrupted approved intent using its
retained source and receipt, without another acquisition or approval.

The file is bounded to 1 MiB and must be regular. Symlink files and symlink
parents are refused. Sibling `.lease`, `.lock`, `.new` and `.state` paths replace
the document's extension. The state directory contains immutable generations and
active/pending references. Choose a parent and a non-reserved extension, in
any case. On Unix, created directories are 0700 and files are 0600. On Windows,
created directories and replacement files receive a protected DACL allowing only
the current user and SYSTEM. Existing parent-directory permissions are preserved.

The projection has a `sources` map from alias to `Source` and an optional
`default` alias. Serialize library types instead of calculating identities or
assembling trust fields yourself. Reads revalidate sources and global scope
rules. The library exposes no project-file adapter: persist that scope in your
application, validate it with `Scope::Project`, then merge it.

## Handle write outcomes

`publish` returns `Publication` after the active reference commits. Its flags
separately report confirmed durability, projection repair, replay and resumption
of unfinished approved intent. Do not interpret post-commit problems as a failed
mutation and run it again. The conditional `replace` adapter reports these as
`NotDurable` or `CommittedRepair`, both of which mean commitment.

Reads use the selected generation even when the projection has missing or
conflicting manual bytes. `Snapshot::projection_ready` exposes disagreement.
Recovery materializes an exact reviewed base or already committed result and
rewrites a deleted projection from the active generation; it preserves
conflicting bytes for the application to reconcile.

To reconcile a manual edit, take a `snapshot` (its `projection_ready()` is
false) and show the person both values: `snapshot.registrations()` is committed,
`read_projection()` is the edited file. Publish the one they choose from that
snapshot. The snapshot binds the exact edited bytes, so a further edit makes the
publication `Changed` instead of overwriting work nobody reviewed. `replace`
cannot show the edit and refuses with `ProjectionConflict(path)`. An interrupted publication
whose reviewed base has since changed never committed, so recovery discards it
and `receipt` returns nothing for its operation; review the change again. `receipt` reads
only the active generation's matching operation, without flushing files or
replaying arbitrary historical mutations.

New kernel leases release ownership on process death and are reused without
deleting marker files. A different legacy marker has an unknowable owner and is
refused with `Locked(path)`. Close other writers and inspect ownership before any
manual repair; the library never guesses that such a marker is abandoned. On a
single-user machine where no process still runs an older writer, deleting that
`.lock` file is the repair; the next write recreates it as a link to its lease.

Writes need a filesystem with hard links; exFAT and some network mounts refuse
them with `Io`. On Unix the lease must belong to the owner of its directory, so
run every writer under the account that owns the configuration directory. See
[Errors and troubleshooting](../reference/errors.md).

[Documentation index](../README.md)
