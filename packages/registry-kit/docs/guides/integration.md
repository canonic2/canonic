# Integration workflow

Use Registry to prepare a reviewed installation, then commit it through your
application's state and activation workflow. The application owns pack validation,
storage locations and the effects of installation.

## 1. Load application state

Load global and project registrations, persisted `Checkpoints` and any existing
lockfile. Construct your built-in source with `Source::pinned` using a public key
and registry ID distributed by your application. Merge registrations with
`Effective::merge`.

See [Sources and trust](sources-and-trust.md) and [Registrations](registrations.md).

## 2. Enroll a new source when needed

Call `Fetcher::discover` for a source the operator wants to add. Show its location,
signing key, registry ID, identity and catalog for review. After approval, add
the source to the selected scope and admit its first catalog.

Persist enrollment and checkpoints through your application workflow. These are
separate operations; the library supplies no transaction across them. Handle an
interrupted operation explicitly rather than treating discovery as enrollment.

## 3. Capture the requested source and pack

Parse input with `selector::parse`, passing a predicate over effective aliases.
For a catalog reference, call `Effective::select`, fetch and admit its catalog,
then resolve its name with `Catalog::lookup`.

Capture the source identity in the root. Dependencies and locked entries must
continue referring to that identity even if an alias or default later changes.
For `pack:<uuid>`, use your installed state instead of a catalog lookup.

See [Find and select packs](selecting-packs.md).

## 4. Build candidate metadata

Acquire candidate artifacts through `Fetcher::artifact` with an explicit byte
limit. Validate each candidate as your pack format, then translate dependencies
with `Dependency::parse` into a `Release<L>`.

Include the candidate versions the resolver may need and releases from reachable
enrolled foreign sources. Authenticate and admit each source's catalog. `resolve`
cannot download missing candidates: supplying only the highest release can miss
a valid closure using an older one.

Keep verified bytes and their `Entry` associated with each `(source identity,
pack ID, version)`. Resolver metadata carries neither paths nor digests.

## 5. Resolve and review

Construct unique `Root<L>` values, then call `resolution::resolve` with candidates
and `Effective::identities()`. Use `()` for `L` when you have no selection levels.

Review the entire returned closure: releases, digests, direct selections,
dependencies and levels. A resolver error returns no partial installation.
It does not grant approval to the returned releases.

Use `resolution::resolve_with_diagnostics` to include failed candidate attempts
in that review. An unusable newest release may lead to selecting an older one.
Compare chosen and installed versions and call out any downgrade during updates.

See [Resolve dependencies](dependencies.md).

## 6. Retain, lock and activate

For every selection, cache verified bytes with `put_expected` and build a
`LockedPack` from its catalog entry and release metadata. Convert omitted
dependency sources to the parent's captured identity. Preserve existing
installed selectors when updating an instance.

Sort and validate entries, then embed them in your application lockfile.
Coordinate lockfile writes, other state and activation according to your own
recovery rules. Cache writes, registration writes and checkpoint persistence
are not one atomic operation.

See [Cache artifacts](cache.md) and [Lock and reproduce installs](lockfiles.md).

## On subsequent runs

Read cached bytes by their locked digests without contacting a registry. For a
cache miss, fetch the recorded source's catalog and require the entry to match
the locked digest before acquiring bytes. Updating an installation is a new
resolution and review, with exact pins retained by the application.

See [Errors and troubleshooting](../reference/errors.md) for recovery.

[Documentation index](../README.md)
