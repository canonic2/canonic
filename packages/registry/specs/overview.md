# Overview

## Problem

Products distribute packs: named, versioned bundles of declarative content,
such as permission policies or agent assets, published by registries. Every
such product needs the same guarantees whatever its packs contain:

- what a registry serves is authentic, current and unchanged;
- the registries an operator trusts are remembered by what they are, not by
  the names the operator gave them;
- a release, once seen, never silently changes;
- dependencies resolve to one compatible set, or the operation is refused;
- installed packs keep working offline;
- an installation is recorded identically on every machine.

`canonic-registry` provides those guarantees once, as a library, so every
product applies the same trust model, limits and resolution rules. It does not
know or inspect what a pack contains.

## Workflow

The library supports this workflow end to end. A product supplies the steps
marked *(product)*; the library owns the others.

1. **Discover** a registry from a location the operator gives. The library
   fetches the registry's key and catalog and authenticates them, and returns
   the source for review without trusting it ([trust](trust.md#discovery)).
2. **Review and register** *(product)*: the operator approves the source. The
   product records it under an alias and keeps the first catalog as a
   checkpoint ([registrations](registrations.md)).
3. **Refer** to a pack by name, `publisher/name`, alias, version or installed
   selector. The library parses the reference and finds its registry
   ([references](references.md)).
4. **Fetch and check** the registry's current catalog. The library refuses a
   catalog that is unauthentic, expired, older than one already seen, or that
   changes a release seen before ([trust](trust.md#checkpoints)).
5. **Choose and acquire** releases. The library picks the requested or highest
   stable version, and refuses artifact bytes that differ from the catalog
   ([acquisition](acquisition.md)).
6. **Resolve** dependencies. The product extracts each release's dependency
   declarations from its own pack format; the library selects one release per
   reachable pack, or refuses ([resolution](resolution.md)).
7. **Review and install** *(product)*: the operator approves the result. The
   product stores the verified bytes and records the installation as lockfile
   entries ([installed state](installed-state.md)).
8. **Reproduce** an installation from the lockfile alone. Cached bytes are
   verified on every read. Missing bytes are fetched again and must match the
   locked digest.

## Vocabulary

- **Pack:** a named unit of declarative content, `publisher/name`.
- **Release:** one immutable version of a pack. Its **artifact** is the bytes
  published for it.
- **Registry:** a local directory or HTTPS origin serving a **catalog** of
  releases ([format](registry-format.md)).
- **Source:** a registry as the library identifies it: its location, plus its
  signing key and registry ID when signed. Its **identity** is derived from
  those ([trust](trust.md#source-identity)).
- **Registration:** an operator-chosen **alias** for a source, in a project
  or global **scope**. Each product has one reserved **built-in** alias for
  its own registry.
- **Enrolled:** a source is enrolled when a registration or the built-in alias
  names it. Only enrolled sources can supply releases.
- **Checkpoint:** what is remembered about a source's catalogs in order to
  refuse rollback and replaced releases.
- **Reference:** a pack as an operator writes it, such as `git`,
  `team/acme/git@1.2.0`, or an installed **selector** `pack:<uuid>`.
- **Level:** an optional ordered setting per installed pack, such as a
  protection profile, that dependencies inherit or map from their parent.
  Products without levels have a single level.

## Responsibilities

The library owns registry mechanics, as specified by the topics in the
[index](README.md). It never executes artifact content, never prompts or reads
a terminal, never reads the wall clock, and never chooses where anything is
stored: expiry decisions take the time as an input, only request deadlines
use a monotonic clock, and every path is an input.

A product that uses the library owns:

- its pack format, the validation of each artifact, and the extraction of
  dependency declarations;
- its built-in registry's location, identity, registry ID and signing key;
- operator review and confirmation, both when a registry is registered and
  before every change to what is installed;
- where registrations, checkpoints, the cache and the lockfile are stored, and
  the lockfile document itself;
- what installed bytes are used for, and publishing a change together with its
  own state.

## Status

Every requirement in these specifications is implemented and verified
([verification](verification.md)), except where a topic lists a gap. No
product depends on the library yet. It is unpublished, in the public
repository, and products use it through path dependencies.
