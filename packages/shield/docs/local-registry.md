# Registry sources and packs

Permission packs contain reusable rules for tools and workflows. Registries
supply those packs; installing a pack does not install its target tool.

Start with discovery below. Pack authors and registry maintainers can skip to
[catalog files](#catalog-files) or [dependencies](#pack-dependencies).

## Find and install a pack

From an initialized project, search the main registry and inspect a result:

```sh
shield pack search main/
```

`main/` searches the built-in registry without installing anything. Copy a
result's full reference in place of `PACK_REFERENCE`:

```sh
shield pack info PACK_REFERENCE
```

Read the description and profile decisions, then add the same reference:

```sh
shield pack add PACK_REFERENCE --profile recommended
shield pack list
```

`pack add` reviews permission changes before confirmation. After acceptance,
`pack list` shows the selection and profile. Omit `--profile` to choose interactively,
with Recommended preselected.

Profile decisions depend on the pack: installing one does not allow every
invocation, and Strict does not change Codex's Ask fallback. See
[policy decisions](policy-decisions.md).

## Change an installed pack

Use `shield pack list` to find an installed selection. Copy its selector, using the
displayed `pack:UUID` if names are ambiguous, in place of `PACK_SELECTOR`:

```sh
shield pack show PACK_SELECTOR
shield pack configure PACK_SELECTOR
```

`show` inspects the retained selection. `configure` walks through any variable
questions and rule overrides, then reviews the change before activation. A
variable supplies a value the pack was written to accept; an override turns a
particular rule off or changes its decision. See
[pack variables and rule overrides](#pack-variables-and-rule-overrides) for
flags and local settings.

To choose another profile for that installed selection, use:

```sh
shield pack add PACK_SELECTOR --profile strict
```

This preserves its retained version and pin and reviews the profile change.
Profile changes use retained artifacts and can work offline.

To update unpinned selections, run:

```sh
shield pack update
```

Review version and permission changes before confirming. Pins are skipped;
resolution or verification failure leaves policy unchanged.

To remove a selection, use:

```sh
shield pack remove PACK_SELECTOR
```

Removal works offline and preserves custom rules. It refuses if a remaining
pack requires that selection; removing a root can also remove dependencies no
longer needed by another selection. Inspect `shield pack list` after confirmation.
The [dependency update and removal reference](#updating-and-removing-dependency-packs)
explains these rules in detail.

## Register a source

Use this section when you need a local catalog or a custom HTTPS registry. You
can use the pinned `main` source without replacing it with your own registration.

Choose registries explicitly, then review permission packs before installing them.
A project registration overrides the same user alias. A project default takes
precedence over the user default. The reserved `main` alias pins
`https://canonic2.github.io/shield-registry/` and the release signing key in the
binary; it installs no implicit policy. The signed main catalog is published
through GitHub Pages. A network-supplied public key cannot replace the pinned root.

```sh
shield registry add acme /path/to/local-registry
shield registry add team https://registry.example.com/ --scope user
shield registry list
shield registry list --scope user
shield registry default acme
shield pack search git
shield pack info acme/git
shield pack add acme/git --profile recommended
shield pack list
shield pack show pack:UUID
shield pack update
shield pack remove pack:UUID
shield registry remove acme
```

Registry mutations ask for terminal confirmation. Removing a registration
preserves installed policy and its source identity. Registry aliases select
sources for discovery; installed selectors identify retained instances.
Use a displayed `pack:UUID` when names are ambiguous.
That selector stays with the installed instance through updates and profile
changes. Removing it retires the selector; reinstalling creates a new one.

New unversioned selections choose the highest engine-compatible stable version.
Shield tries older stable releases when newer ones use an unsupported engine or
policy format. Invalid digests or artifact identities stop acquisition. When you add a
new pack without `--profile`, Shield asks for its profile after resolving the
release, with Recommended preselected; Enter keeps it. Dependencies follow the
profile you choose. An exact `@version` pins that release. Adding an installed selector
without a version preserves its version and pin; `--profile` changes its profile
without requiring the source to be online. Update skips pinned releases and
reviews all applicable changes in one transaction. Removal works offline.

## Local catalog

Registry maintainers define catalogs as described below. For project settings,
see [pack variables and overrides](#pack-variables-and-rule-overrides).

### Suggestions for commands

Claude and Codex can suggest packs for commands that Ask. A suggestion names
the source/publisher/pack and changes no permissions. Inspect its profiles with
`pack_info`; a relevant pack may still Ask or Deny the invocation.
Its summary includes bounded examples of declared Allow, Ask and Deny outcomes
for each profile, with conditional rules and unmet-requirement outcomes marked.
The text preview shows recommended-profile examples. Review the intended
profile's retained approval requirements alongside its allowances before
describing installation benefits. Other active rules, conditions and requirements
still determine effective permissions; Ask follows the host's approval or
configured fallback. Omitted examples are marked in `missing`/`truncated`.

For one candidate, the agent reviews it and offers installation once per
conversation after the task, even if the command ran under Codex's fallback.
It explains the scope, proposes the change, and submits `policy_apply`.
The host approval prompt is the installation question. Multiple candidates
need selection first. See the [MCP walkthrough](project-mcp.md#reviewing-an-available-pack).

Suggestions use the latest locally retained catalog metadata. Run `shield pack search
main/` in the project to authenticate and retain the main catalog, or replace
`main` with an enrolled alias. Hooks never refresh catalogs or download packs.
Already installed instances and removed sources are excluded.

Catalog v2 adds optional `commands`, for example
`"commands":{"acme/runtime":["node","npm","npx"]}` alongside its `packs`.
Each ID needs a listed stable release. Lists contain 1–16 unique command basenames
(at most 64 ASCII letters/digits/`._+-`, starting alphanumeric). These hints are
authenticated with the catalog, and carry relevance rather than permission.
V1 catalogs remain supported and supply no command hints. V1-only clients cannot
read v2 catalogs; upgrade clients before publishing v2. Checkpoints with a command
index also require the updated client; downgrading is not supported for that state.

### Catalog files

A local directory contains `catalog.json` and declarative pack artifacts:

```json
{"schema_version":1,"packs":[{"id":"acme/git","version":"0.1.0","path":"packs/git.yml","sha256":"<64-character SHA-256>"}]}
```

Replace the checksum placeholder with the artifact digest. Artifact paths use
ASCII letters, digits and `-._~/` without empty, `.` or `..` segments. An optional
`expires_at` must be a whole-second UTC timestamp. An optional `sequence` is
retained like a signed catalog's: once a catalog carries one, later catalogs
must keep a sequence that never decreases. Reads are confined to
the canonical enrolled directory. Pack identity/version must match the catalog.
Artifacts contain policy data, never installers. Same-version replacement bytes
are refused for retained selections. Distinct registries may supply the same pack
name; their retained identities keep those selections separate.

## Custom signed HTTPS catalog

A custom HTTPS directory serves `catalog-key.pub` (64 hexadecimal characters),
`catalog.sig` (128 hexadecimal characters), `catalog.json`, and the catalog's
relative artifact paths. The key is Ed25519; the detached signature covers the
exact catalog bytes. The catalog adds `registry_id`, integer `sequence` and
UTC `expires_at` metadata.

Enrollment verifies ordinary TLS, presents the origin/key for operator review,
and retains that key and registry identity. Acquisition verifies signatures,
expiry and retained monotonic sequence/digest checkpoints before using releases.
Artifacts must match signed digests. Redirects, unsafe paths, unsigned metadata
and source identity changes are refused. Offline evaluation uses retained bytes.
This custom-source format does not supply the main registry's release trust root.

## Pack dependencies

Packs enable `dependencies-v1` and declare dependencies in their exact artifact bytes:

```yaml
schema_version: 1
kind: pack
id: acme/workflow
version: 1.0.0
description: Acme workflow permissions
license: UNLICENSED
maintainers: [Acme]
engines: {shield: '^0.8'}
capabilities: [structured-rules-v1, dependencies-v1]
dependencies:
  - pack: acme/git
    requirement: '^1.0'
rules: []
```

Each dependency uses publisher/name and a SemVer requirement. It inherits the
parent's enrolled source unless `registry` names another explicitly enrolled source
identity. Use the `identity` shown by `shield registry list`, rather than its alias or
the operator's default. For example:

```yaml
dependencies:
  - pack: acme/git
    requirement: '^1.0'
    registry: '<captured enrolled source identity>'
```

Enroll cross-source registries before acquisition. A pack cannot register a
source or choose its URL or signing key. Identical pack names from different
sources stay distinct; use `pack:UUID` when names are ambiguous.

Dependencies inherit the parent's profile unless a complete `profiles` mapping
selects one for each parent profile. The mapping must become at least as
restrictive from Loose to Strict. Shared dependencies use the strongest required
profile. A directly selected dependency keeps its explicit profile and reports
a conflict if that profile is too weak. Omit both `dependencies` and
`dependencies-v1` when unused; optional fields cannot be null.

### Resolution and review

Add and MCP pack-add proposals verify the full dependency set before review.
A new unversioned selection chooses the highest compatible stable set; existing
roots keep their versions, and exact requested versions stay pinned. Cycles,
conflicts, missing releases, bad digests, or changed source identity leave
active policy unchanged.

The review lists selections, dependency/profile changes, and incoming constraints.
Confirmation activates the complete candidate together. `shield pack list` and
`.shield/shield.lock` retain direct-selection flags, versions, profiles, source
identities, digests, and dependency edges. Manual review rejects incomplete or
incompatible sets. Promoting a dependency to a direct selection requires a
profile satisfying all its parents.

### Source verification

Dependencies can span enrolled local and signed HTTPS registries. Resolution
uses one checked catalog snapshot per source and verifies every artifact digest.
HTTPS acquisition uses the saved origin, identity, and signing key; it never
fetches a replacement key. Signature, expiry, rollback, path, size, and timeout
checks apply throughout. Redirects, unknown identities, alias substitution, and
changed source bindings are refused.

Captured proposals can be confirmed offline. Profile changes and removal also
use saved artifacts after registry removal. Dependencies accept no settings.
Resolution is limited to 64 packs and bounded catalog, byte, and search budgets.
Pack review runs no installers or pack code.

## Pack variables and rule overrides

Some packs declare variables, such as the package manager or cloud profile the
project uses. `shield pack add` asks one question per variable after resolving the
pack, in the order the pack declares them, preselecting a value when a
suggested file such as `pnpm-lock.yaml` exists. An optional variable lists
leaving it unset as its first answer, which turns its checks off; a text value
is typed into the question's last row. A pack can word each question and skip
it depending on an earlier answer. With more than one question, the answers
are shown together before saving, and choosing one asks it again. Bind values directly with
`shield pack add acme/node --set package_manager=pnpm`. A list value is a JSON array
or comma-separated items; booleans are `true` or `false`.

`shield pack configure acme/node` reopens the questions for an installed
selection, then lists the pack's rules with what each one decides, with
overridden rules already marked. Press Space to mark the rules to change and
Enter to continue; each marked rule then asks whether to turn it off or give it
another decision, and unmarking a rule returns it to the pack's decision. Enter
with nothing marked keeps every rule as the pack decides, and long lists can be
searched. Its flags change values and overrides without questions:

```sh
shield pack configure acme/node --set package_manager=pnpm --unset registry
shield pack configure acme/node --override npm-publish=deny --override lockfile-check=off
shield pack configure acme/node --override scripts-require-ci=on_unmet:ask --clear-override npm-publish
```

Add `--local` to keep a change on your machine only: `shield pack add --local` adds a
pack just for you, and `shield pack configure acme/aws --set profile=acme-read-only --local`
sets a personal value. After the guided questions, one question chooses where
the answers are saved: as the pack suggests for each value, all shared with the
project, or all only yours. `pack configure --local` saves them locally without
asking, and a pack added with `--local` keeps everything local. Local settings
live in `.shield/config.local.yml` (with `rules.local.yml` and
`shield.local.lock`), which Shield adds to `.shield/.gitignore`. A local entry
can also turn off a shared pack for you with `disabled: true`; edit the file and
review it with `shield policy review`. Agents cannot edit these files.

An override turns a pack rule or precondition `off` or replaces its decision;
it cannot add rules or change conditions. Values and overrides are stored with
the selection in `.shield/config.yml`. Every change goes through the ordinary
review, which lists changed values and overrides and marks overrides that widen
permissions. Updates keep values and overrides; review lists any that the new
release no longer declares, and they are removed. Shield asks again when an
update adds a required variable or no longer accepts the bound value. Agents can
propose the same changes with the MCP `pack_configure` change.

## Updating and removing dependency packs

`shield pack update` resolves one complete compatible closure for all tracking roots;
direct selections from the built-in `main` registry use its pinned source and
need no additional registry enrollment.
Tracking direct selections update to their installed version or a newer release.
`shield pack update pack:<uuid>` updates that instance and its reachable dependencies.
Direct profiles and pinned versions stay fixed. Updating a pinned instance is a
no-op. Dependencies reachable only from skipped pinned roots retain their versions;
shared dependencies reached by tracking roots can update within every parent's
constraints. An unpinned dependency can also be updated explicitly, subject to
all parent constraints. Source enrollment is checked by captured identity, so
renaming an alias cannot retarget an installed instance.

Review shows each pack added, removed, updated or given another profile, and how
command decisions change; `shield pack list` shows the resulting versions, profiles
and dependency edges. Confirmation activates the whole candidate together. Failed verification,
incompatible closures, canceled review and stale proposals preserve active policy.
MCP `pack_update` proposals capture the same verified closure for operator review;
confirmation can use those captured bytes after the source becomes unavailable.

`shield pack remove pack:<uuid>` works offline using retained artifacts. If another
remaining pack requires the instance, removal refuses and names its dependents.
Removing a root prunes only dependencies no longer reachable from remaining roots.
Shared dependencies and explicitly selected packs remain. MCP `pack_remove`
proposals use the same preparation and confirmation boundaries. Custom rules are
preserved across both workflows.
