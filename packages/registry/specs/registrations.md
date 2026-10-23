# Registrations

A registration gives an enrolled [source](trust.md#source-identity) an alias
that operators can type. Aliases exist for discovery only: installed packs and
dependencies always refer to source identities, so renaming or removing a
registration never changes what is installed.

## Aliases and scopes

An alias is 1–64 lowercase ASCII letters, digits or hyphens. Registrations
live in two scopes, each holding a [bounded](limits.md#registrations) set of
aliases and an optional default alias:

- **Global:** one user's registrations for one product, stored in the
  [global file](#global-file).
- **Project:** one project's registrations, stored by the product.

Rules for a scope:

- An alias appears once per scope, and never as the built-in alias.
- A registered source carries its [derived identity](trust.md#source-identity);
  a source with a chosen identity, as only built-in sources have, cannot be
  registered.
- Removing an alias clears the scope's default if it named that alias.
- A default must name a registered alias when it is set. As stored, a global
  default must name a global alias or the built-in alias. A project default
  is only checked to be an alias, because the global alias it names may
  later be removed.
- A stored scope is checked against these rules when it is loaded. The
  [global file](#global-file) does this itself; a product that loads a
  project scope checks it before use.

## Effective registrations

The registrations in effect combine the built-in source, the global scope and
the project scope:

- Both scopes are validated when they are combined, however the product
  loaded them, so no registration in effect can use the built-in alias or
  claim another source's identity. The global scope is checked first, and
  the first refusal is returned.
- Project aliases shadow global aliases of the same name.
- The built-in alias is always present and never shadowed.
- The default is the project's, else the global one, else the built-in alias.
- Selecting an alias, explicit or the default, that is not registered is
  refused (`Missing`); there is no fallback to another registry.
- Each alias reports its scope: `builtin`, `global` or `project`.
- The identities of all registered sources form the enrolled set used by
  [resolution](resolution.md).
- A source identity maps back to the first registration, in alias order,
  whose source has it, together with that registration's scope.

## Global file

The global scope is stored as JSON at a path the product chooses; a bare file
name lives in the current directory. The lock and temporary files sit beside
it, named by replacing its extension with `.lock` and `.new`.

Reading:

- A missing file is an empty scope.
- A symbolic link, a file that is not a regular file or is over its
  [bound](limits.md#registrations), an invalid document, an invalid source and
  any rule violation are refused. A FIFO or device is refused without
  waiting. On Unix the file is opened without
  following a link, so a link swapped in after the check is refused too.

Replacing the file with a new value is conditional on the operator's review:
it succeeds only if the file still holds the value that was reviewed.

1. The new value is checked against the scope rules.
2. A file path whose extension is `.lock` or `.new` in any letter case, or
   that has no parent, is refused. Case is compared after Unicode lowercasing,
   as case-insensitive filesystems compare names, so the file can never be its
   own lock or temporary file.
3. The parent directory is created if missing, private to the user on Unix
   (0700). A parent that is a link is refused.
4. An exclusive lock file is created; if it already exists, the write is
   refused and the lock path reported.
5. If the file no longer holds the reviewed value, the write is refused. If
   the new value would serialize beyond the file's
   [bound](limits.md#registrations), which escaped characters can cause
   even for valid registrations, the write is refused, so a written file can
   always be read back.
6. A leftover temporary file from an interrupted writer is removed. The new
   value is written to the temporary file (0600 on Unix), synced and renamed
   over the file. On Unix the directory is then synced, so the new value
   survives a crash. Both syncs are the ones
   [cache writes](installed-state.md#artifact-cache) receive.
7. The lock is removed, whatever the outcome.

A lock left behind by a crashed writer refuses every later write until it is
removed; the refusal names its path.

Once the rename in step 6 succeeds, the new value is committed and is never
rolled back. A refusal before that point leaves the file unchanged. After it,
only two outcomes are reported as errors, and both mean the new value was
written: the directory sync failed (`NotDurable`), or the lock could not be
removed (`LockRemains`, which takes precedence and names the lock to remove).

A lock that cannot be removed is always reported, never hidden behind another
refusal. When nothing was written and the lock also could not be removed, the
refusal and the lock are reported together (`RefusedLockRemains`).

The stored form is `{"sources": {alias: source}, "default": alias}`, with
`default` omitted when the scope inherits it.

## Refusals

| Refusal (`RegistrationError`) | Cause |
| --- | --- |
| `InvalidAlias` | Not 1–64 lowercase letters, digits or hyphens |
| `Reserved(alias)` | The built-in alias cannot be registered, removed or appear in a stored scope |
| `Identity(alias)` | The registered source's identity is not derived from what it is |
| `Duplicate` | The alias is already registered in this scope |
| `Missing(alias)` | The alias is not registered |
| `Limit` | The scope is full, or a stored scope holds more than its bound |
| `InvalidDefault` | A stored global default names neither a global alias nor the built-in alias |
| `Malformed` | The file is not a regular file (including a link swapped in after the check), is too large, or is not a valid document or source |
| `TooLarge` | The new value would serialize beyond the file's bound |
| `Path` | The file path has no parent or ends in `.lock` or `.new`, in any case |
| `Link` | The file or its directory is a symbolic link |
| `Changed` | The file changed after it was reviewed |
| `Locked(path)` | Another writer holds the lock, or a crashed one left it |
| `NotDurable` | The new value was written, but its directory could not be synced, so a crash may revert it |
| `LockRemains(path)` | The new value was written, but the lock could not be removed |
| `RefusedLockRemains { refusal, lock }` | Nothing was written, for the refusal it carries, and the lock could not be removed |
| `Io` | A filesystem failure |

## Decisions

- **Aliases never identify installed packs.** Operators rename and remove
  registrations; installations must not move with them.
- **Project shadows global, built-in shadows both.** A project can use its own
  registry under a familiar name, but no configuration can impersonate the
  product's own registry.
- **Conditional replacement.** The operator approves a specific change; a
  concurrent change by another process makes that approval stale.
- **Lock and temporary names replace the extension.** They stay recognizable
  beside the file. Paths that would collide with them are refused.
