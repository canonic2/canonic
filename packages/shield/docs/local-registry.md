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
shield pack add PACK_REFERENCE
shield pack list
```

`pack add` reviews permission changes before confirmation. After acceptance,
`pack list` shows the selection and profile. New selections default to
Recommended. Use `--profile` to choose another profile.

Profile decisions depend on the pack: installing one does not allow every
invocation, and Strict does not change Codex's Ask fallback. See
[policy decisions](policy-decisions.md).

For GitHub API queries, see [GitHub GraphQL reads](#github-graphql-reads) for
the distinction between engine analysis and installed pack permissions.

## Change an installed pack

Use `shield pack list` to find an installed selection. Copy its selector, using the
displayed `pack:UUID` if names are ambiguous, in place of `PACK_SELECTOR`:

```sh
shield pack show PACK_SELECTOR
shield pack configure PACK_SELECTOR
```

`show` inspects the retained selection. `configure` walks through any variable
questions and rule overrides, then reviews the change before writing it. A
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

To review updates for all installed direct selections, including pinned packs, run:

```sh
shield pack update
```

From an agent, the matching MCP tool is `pack_update`: `{}` selects all
direct packs and `{"reference":"gate"}` selects one. Append `@VERSION` to select
an exact release. Add `review: true` to request human approval of the update.

Review version and permission changes before confirming. Updated packs keep
their profiles and pin mode at the new versions. Updates run only on demand;
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

## Check for pack updates

`shield status` and `shield pack list` show cached availability for all direct
selections, including pinned packs. Pins keep the installed version fixed until
an explicit reviewed update; they do not hide newer releases.
A newer advertised release may need a newer Shield engine,
different settings or compatible dependencies; `shield pack update` prepares the
actual candidate and shows permission changes for approval.

Refresh the main registry's metadata explicitly, then inspect installed packs:

```sh
shield pack search main/
shield pack list
```

Replace `main` with an enrolled alias for a custom source. Search downloads
catalog metadata without downloading pack artifacts or installing rules. Normal
status/list requests and hooks make no update-related network requests.

Metadata stays fresh for 24 hours, bounded by its signed expiration. Missing or
old metadata means availability is unknown. Inspection retains observation times
and distinguishes stale evidence from fresh releases. Dependency-only selections
do not produce notices. An empty scope reports “No direct pack selections to check.”

Interactive status/list and completed pack changes may also print a short notice
on stderr. Notices are limited to once per project per day; the same installed
and advertised version pair is suppressed for seven days. JSON and piped
commands receive no unsolicited notice. `--json status`, `--json pack list` and
`--json pack show PACK_SELECTOR` retain structured `pack_updates` inspection.

Set `SHIELD_PACK_UPDATE_CHECK=off` in the environment used to launch Shield to
disable unsolicited notices; `cached` is the default. Explicit inspection and
registry operations remain available. No automatic background refresh runs.

## GitHub GraphQL reads

GraphQL analysis and permission are separate. The engine implements `graphql-v1`
for custom rules and packs that declare it. The published GitHub 0.3.0 pack does
not use that capability: unscoped raw API calls require review, and setting its
optional `repository` boundary excludes raw API calls. Updating the binary alone
does not add query permissions to the installed pack.

Use `shield pack list` and `shield pack show PACK_SELECTOR` to inspect the
installed version, profile, settings and rules. Review available permission
changes through [the pack update workflow](#change-an-installed-pack). For
authoring custom GraphQL rules, see the [GraphQL reference](rule-reference.md#graphql-requests);
to investigate a refusal, see [troubleshooting](troubleshooting.md#a-read-only-github-graphql-query-is-blocked).

## Register a source

Use this section when you need a local catalog or a custom HTTPS registry. You
can use the pinned `main` source without replacing it with your own registration.

Choose registries explicitly, then review permission packs before installing them.
Project registrations are the `registries` [setting](policy-files.md#settings):
`shield registry add` writes them to your `.shield/config.local.yml`, and a
`registries` key committed in `config.yml` shares them. A project registration
overrides the same user alias. A project default takes precedence over the user
default. The reserved `main` alias pins
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

These CLI registry mutations ask for terminal confirmation. From an agent, use
`registry_add` with `{"name":"team","location":"https://registry.example.com/"}`.
Using an alias already registered in that scope requests a replacement review.
Compare its previous and proposed locations and trust before accepting. The
alias's default selection and installed packs stay intact; installed packs keep
their original source identity. A project registration shadows the same alias in
your user configuration. Removing it reveals that user source, which the review
identifies even when the default alias stays the same.

User-scope changes publish a committed generation and retain their receipt.
After an interruption, retry the same command: Shield reconciles an unfinished
approved change using its captured source. A settled add can check the presented
source again; an unchanged source reconciles its receipt, while a new key or
location requires replacement review. A successful receipt can still report
`repair_needed` or unconfirmed `durability`; that means the change committed and
needs reconciliation, not another installation. Shield preserves conflicting
manual edits in `~/.shield/registries.json` and keeps the committed registrations
active. Inspect both before correcting that file or preparing another global change.

Shield validates the discovered catalog, then shows a human form with its
canonical location, registry identity, signing-key SHA-256 fingerprint and trust
basis. HTTPS enrollment trusts the reviewed origin/key on first use; a signature
does not independently prove publisher identity. Local sources show their
canonical directory and unsigned trust basis. Relative directories resolve
against the selected project.

If an agent does not know the alias or location, it can omit that field from
`registry_add` and set `review: true`. Shield collects the missing details,
then starts a separate trust review. A host that needs a separate call returns
an opaque reference to the captured details. Supplied fields stay fixed. To
release captured details, call `registry_add` with the returned `review_ref` and
`discard: true`.

The default scope is `project`. Set `"scope":"user"` to change the user's
registry configuration for other projects too; the form states that effect.
Declining, cancelling or closing the connection leaves the source unenrolled.
Discovery alone grants no trust or permissions. If a dependency needs an
unenrolled source, call `registry_add` for it and retry the pack request after
acceptance. Pack permissions still need their own review. The host must support
Shield's human confirmation forms.

Removing a registration
preserves installed policy and its source identity. Registry aliases select
sources for discovery; installed selectors identify retained instances.
Use a displayed `pack:UUID` when names are ambiguous.
That selector stays with the installed instance through updates and profile
changes. Removing it retires the selector; reinstalling creates a new one.

New unversioned selections choose the highest engine-compatible stable version.
Shield tries older stable releases when newer ones use an unsupported engine or
policy format. Invalid digests or artifact identities stop acquisition. New
packs default to Recommended and dependencies follow their parent's profile.
An exact `@version` pins that release. Adding an installed selector
without a version preserves its version and pin; `--profile` changes its profile
without requiring the source to be online. Explicit update includes pinned releases,
keeps their pin mode, and reviews all applicable changes in one transaction.
Removal works offline.

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

Shield reads flat v1/v2 catalogs and indexed v3 catalogs. V3 discovery lists
current pack heads; authenticated history pages retain old versions. Exact pins
fetch only their version branch, while dependency resolution fetches selected
packs' history within byte, page and search budgets. Signature, expiry,
anti-rollback and retained-digest checks apply to both formats. V1/v2-only Shield
builds require an upgrade before a registry switches to v3.

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
Confirmation writes the complete change together. `shield pack list` and
`.shield/shield.lock` retain direct-selection flags, versions, profiles, source
identities, digests, and dependency edges. Hand-edited files with incomplete or
incompatible sets make policy unavailable; supported native requests then need
[exact in-session review](project-review.md#allowing-one-blocked-request).
Promoting a dependency to a direct selection requires a
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

Packs can discover installed executable paths during add, update and configure.
For example, a Rust pack with executable discovery checks `CARGO_HOME/bin` and
`~/.cargo/bin`. The review names the exact paths and saves them locally. Existing
bindings are preserved, and discovery never executes a tool. Discovery skips
executables in the project, approved write folders and system temporary folders,
including symlinks into them. Through MCP, call
`pack_configure` with the installed reference and no edits to review missing
discovered bindings; use `variables` when you want to edit particular settings.

When a single command using an absolute executable path matches no rule,
`suggestion_list` can point to an unset setting in an installed pack. For example,
it can recommend binding `cargo_executable` locally when the selected Rust
profile covers that Cargo command and the exact path matches a declared
discovery candidate outside writable folders. The next action prepares a `pack_configure`
proposal for review. Listing the suggestion grants no permission.

Some packs declare variables, such as the package manager or cloud profile the
project uses. `shield pack add` asks one question per variable after resolving the
pack, in the order the pack declares them, preselecting a value when a
suggested file such as `pnpm-lock.yaml` exists. An optional variable lists
leaving it unset as its first answer, which turns its checks off; a text value
is typed into the question's last row. A pack can word each question and skip
it depending on an earlier answer. The footer shows the controls for choosing,
typing, continuing and going back. With more than one question, the answers
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
can also replace a shared pack's release or profile: use
`shield pack add acme/aws --profile strict --local`, adding `@VERSION` to select
another release. `shield pack remove acme/aws --local` turns off a shared pack
or removes your local selection. Removing a local replacement keeps the shared
selection turned off; delete its local `disabled: true` entry and review with
`shield policy review` to re-enable it. Shared configuration and lock files keep
their contents. Agents cannot edit these files.

An override turns a pack rule or precondition `off` or replaces its decision;
it cannot add rules or change conditions. Values and overrides are stored with
the selection in `.shield/config.yml`. Every change goes through the ordinary
review, which lists changed values and overrides and marks entries that may widen
permissions. Its permission comparison describes the whole proposal, including
other rules; incomplete comparisons stay unknown even when they show a concrete
example. Updates keep values and overrides; review lists any that the new
release no longer declares, and they are removed. Shield asks again when an
update adds a required variable or no longer accepts the bound value. Agents can
propose the same changes with the MCP `pack_configure` change.

Decision explanations include each matched pack rule's selected profile,
authored default, decision before the effective override, final decision and
whether the pack, shared configuration or local configuration supplied it.
The same evidence accompanies failed preconditions and shared command bodies.

## Updating and removing dependency packs

`shield pack update` resolves one complete compatible closure for all direct roots;
direct selections from the built-in `main` registry use its pinned source and
need no additional registry enrollment.
Direct selections update to their installed version or a newer compatible release.
`shield pack update pack:<uuid>` updates that instance and its reachable dependencies.
Direct profiles and pin modes stay fixed; an updated pin names the resulting version.
Unselected roots retain their exact versions during a targeted update.
Reachable dependencies can update within every parent's constraints.
A dependency can also be updated explicitly, including an exact version such as
`shield pack update acme/child@1.1.0`, subject to all parent constraints. It
remains a dependency with its derived profile; a conflicting or missing release
refuses the update without changing policy.
Source enrollment is checked by captured identity, so
renaming an alias cannot retarget an installed instance.

Review shows each pack added, removed, updated or given another profile, and how
command decisions change; `shield pack list` shows the resulting versions, profiles
and dependency edges. Confirmation writes the whole change together. Failed verification,
incompatible closures, canceled review and stale proposals leave the policy files unchanged.
MCP `pack_update` proposals capture the same verified closure for operator review;
confirmation can use those captured bytes after the source becomes unavailable.

`shield pack remove pack:<uuid>` works offline using retained artifacts. If another
remaining pack requires the instance, removal refuses and names its dependents.
Removing a root prunes only dependencies no longer reachable from remaining roots.
Shared dependencies and explicitly selected packs remain. MCP `pack_remove`
proposals use the same preparation and confirmation boundaries. Calling it
without a reference lets you select 1–16 installed packs together, with joint
dependency checks and a separate final policy approval. This form supports at
most 64 installed choices and needs a multiple-selection-capable MCP client;
explicit single references remain available. Custom rules are
preserved across both workflows.

## Delegated signing roles and revocations

If an enrolled custom HTTPS source publishes Shield's versioned root and signed
revocation metadata, review its delegated roles from a project:

```sh
shield registry trust acme
```

The confirmation shows the original signing-key fingerprint, root-role
fingerprints and thresholds, expiry and revocation sequence. This trust applies
to that source across your workspaces. It grants no pack permissions; installing
policy still has its separate review. Registered agents use `registry_trust` for
the same confirmation. The built-in `main` registry continues to use its binary
pin and does not accept this command.

After adoption, acquisitions require current signed metadata and authenticated
root transitions. Missing metadata does not switch the source back to its old
trust mode. Installed policy works offline unless an exact artifact is known
revoked. A known revocation blocks affected policy until you update or remove
the pack, including copies and restored checkouts. Diagnostics and repair remain
available.
