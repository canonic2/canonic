# Registrations

Use `Registrations` for one scope's aliases and default. Use `Effective` to merge
the built-in source, user-global scope and optional project scope.

## Merge scopes

```rust
use canonic_registry::{
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

`GlobalFile` reads and conditionally replaces a JSON document at your chosen path.
A missing file reads as empty registrations.

```rust
use canonic_registry::{registrations::GlobalFile, source::Source};
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

Replacement validates `after`, takes an exclusive sibling lock, rereads the file
and writes only if it still equals `before`. On `Changed`, reread and rebuild
the proposed change; obtain review again if what was approved has changed.

The file is bounded to 1 MiB and must be regular. Symlink files and symlink
parents are refused. The `.lock` and `.new` sibling paths replace the document's
extension: `registries.json` uses `registries.lock` and `registries.new`.
Choose a path with a parent and an extension other than `.lock` or `.new`, in
any case. On Unix, created directories are 0700 and files are 0600.

The serialized shape has a `sources` map from alias to `Source` and an optional
`default` alias. Serialize library types instead of calculating identities or
assembling trust fields yourself. Reads revalidate sources and global scope
rules. The library exposes no project-file adapter: persist that scope in your
application, validate it with `Scope::Project`, then merge it.

## Handle write outcomes

Most refusals leave the file unchanged. Two errors mean replacement committed:
`NotDurable` means the subsequent directory sync failed, and `LockRemains`
means the lock could not be removed. Read the current state before deciding
whether to retry. `RefusedLockRemains` means nothing committed and carries both
the original refusal and the lock path.

`Locked(path)` can indicate an active writer or a lock left after a crash.
Verify that no writer still owns it before removing a stale lock. See
[Errors and troubleshooting](../reference/errors.md).

[Documentation index](../README.md)
