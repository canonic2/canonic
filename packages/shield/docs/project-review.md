# Project policy, approvals and history

Use this guide to manage an initialized project: inspect policy, connect agents,
review changes, and investigate requests. If this is your first setup, follow
[your first project](getting-started.md) first. For the meaning of Allow, Ask,
and Deny, read [policy decisions](policy-decisions.md).

## Choose a task

| You want to… | Start with |
| --- | --- |
| See what is active | `shield status`; use `shield doctor` for a problem. |
| Connect or repair an agent | [Native hooks and MCP setup](#native-hooks-and-mcp-setup) |
| Add a reusable permission pack | [Find and install a pack](local-registry.md#find-and-install-a-pack) |
| Change one project-specific permission | [Custom rules](#custom-rules-and-proposed-changes) |
| Review a change prepared by an agent | `shield policy review`, or the host's Shield approval prompt. |
| Let location rules include another directory | [Additional access paths](#additional-access-paths) |
| Understand a past request | [Request history](#request-history) |

Inspection commands report state. Commands that change policy show a review
and require confirmation. The examples below run from your project directory;
replace illustrative IDs and paths with values from your own project.

## Inspect your project

Run Shield in a project directory, or add `--project /path/to/acme`. Inspection
finds the nearest enrolled ancestor; a nearer enrollment owns its own boundary.

```sh
shield status
shield doctor
shield policy show --json
shield rule list --json
```

`status` is the everyday overview. Use `doctor` when something looks wrong: it
checks policy loading, registry checkpoints, retained pack sources, and the
binary your agents run, and gives repair commands when needed. JSON output
provides full structured details for scripts and agents.

If the complete status report cannot be read, doctor returns partial metadata
with `missing: [complete_status]` and a `status` check. Unreadable retained policy
source appears as a `policy_source` check. These reports require a valid authority
checkpoint; they do not repair or replace damaged source. Restore lost retained
source before using policy changes to recover the project.

## Initialize or revisit setup

Run `shield init` to review and confirm project policy. It offers the same
guided agent setup as `shield mcp setup`. Mark the agents to configure, or continue
with none selected. `--agent claude` or `--agent codex`, repeatable, selects
directly; `--no-setup` skips agent setup.

New projects acquire the signed main baseline with Recommended preselected.
Choose another profile during setup, pass `--profile strict`, or use
`--no-baseline` to skip it. Download or verification failure leaves a new
project unenrolled. Offline setup can use `--no-baseline` with supplied artifacts.

Existing packs, rules, profiles, and removals are preserved. Use
`shield pack remove baseline` for later removal. See
[terminal controls](operator-experience.md).

## Native hooks and MCP setup

Add the agent you want to use, then restart and trust it:

```sh
shield mcp setup --agent claude
shield mcp setup --agent codex
```

Each command installs hooks and MCP while keeping the other agent registered.
`shield mcp setup` without `--agent` asks which agents to install or repair, or
resumes an interrupted confirmed plan. `shield mcp setup --remove` asks which
agents to remove, or takes `--agent`, and preserves policy, approvals, and
history. Add `--dry-run` to see the review without writing anything. Edited owned entries report a conflict rather than
being overwritten.

New Codex setups let valid Ask requests proceed by default, including unmatched
requests and Strict-profile Asks. Existing Block or Allow choices are preserved;
there is no Block picker or flag. Deny, protected files, and unsupported tools
still block. Codex's own sandbox and approvals also apply.

Status and doctor report the effective Ask fallback. Missing authorization or
an interrupted setup can report Block; finish the plan with `shield mcp setup`.
See [Ask handling](policy-decisions.md#what-happens-after-ask) and the
[integration guide](claude-project.md) for loading and coverage.

## Protected files

```sh
shield protection set
```

By default, native hooks ask before agents read `.mcp.json` and agent hook
settings (`.claude/settings.local.json`, `.claude/settings.json`, `.codex/hooks.json`, `.codex/config.toml`),
which may hold credentials, and block agents from changing them or the `.shield`
policy files.

The same settings protection covers the adapter's configured agent home,
including a relocated home. Verified session-output reads retain their separate
scope; they do not permit configuration writes.

`protection set` asks about each group and confirms the result:

| Group | Default | Other choice |
| --- | --- | --- |
| Reading agent settings | Ask, or block where the agent cannot prompt | Allow without asking |
| Changing agent settings | Block | Allow; the agent can then remove Shield's hooks or the host confirmation that MCP policy changes rely on |
| Changing `.shield` policy files | Block | Allow; edits still apply only after `shield policy review` |

`.shield/review/`, compiled caches, the `.shield` folder itself and
`.shield/.gitignore` stay protected whatever you choose, and agents still cannot
run `shield`. The choice is kept with the project's review state; agents cannot
change it. `status` lists any group you have opened. An invalid saved choice
blocks every native request until you run `protection set` again.

## Custom rules and proposed changes

Custom rules express project-specific permissions. For a worked example, see
[your first rule](policy-rules.md#your-first-rule).

### Add, edit, or remove a custom rule

```sh
shield rule add
```

Choose the matching form and decision, then review its scope. After confirming,
find the rule's ID with `shield rule list --json`. To edit or remove a rule named
`inspect-status`, use the appropriate command:

```sh
shield rule edit inspect-status
shield rule remove inspect-status
```

The default **Complete request** form binds normalized input, vendor fields,
cwd, environment, and executable. **Exact command arguments** binds an executable
and argument array to the project cwd, with broader vendor/environment scope.
Both ask for a decision, reason, and optional UTC expiration. **Structured rule**
accepts a complete rule in the [rule format](policy-rules.md).

Editing shows the existing rule; empty answers preserve its conditions.
Confirmation activates the validated candidate; cancellation leaves policy unchanged.

### Review an agent's proposal

Agents can propose packs, rules, domain permissions, or folder access. Check
whether a proposal covers one invocation or a broader set of requests. To
review pending proposals yourself, run:

```sh
shield policy review
```

Proposal review is the default when proposals are pending. They appear oldest
first, with the change, agent's reason, originating command, and permission
comparison. Tab opens the complete change, including pack rules. Choose Accept,
Reject, or Leave pending; Accept asks for final confirmation.

Agents can also submit a proposal's exact `apply_arguments` to
`policy_apply` for host approval. `policy_receipt` reads the
committed receipt. See [MCP review](project-mcp.md#proposals-and-operator-review).

Changes to policy, setup, registry settings, or cache location can invalidate
a proposal. Ask the agent to prepare a fresh one, even if the policy revision
has not changed. Independently confirmed protected-file settings remain current.
For successive proposals, wait for each result before preparing the next.

### Activate a manual file edit

Project policy files represent the committed snapshot. After editing configuration
or rules, run `shield policy review`, choose `manual`, and confirm the reviewed changes.
Inspect `shield rule list --json` for the active result. Changes during confirmation
abort publication. Use `shield pack add` to acquire releases rather than editing
cached pack artifacts.

Shield preserves comments, blank lines, and layout outside changed entries.
Removing a rule or pack also removes its preceding comments. If a change cannot
be made in place, such as reordering rules, Shield rewrites below the opening
comments. The review identifies files that will be rewritten or lose comments.

## Additional access paths

If your agent needs a reference folder outside the project, you can add that
folder to the roots used by location rules. Adding a root does not itself allow
every operation there; the active rules still decide what the agent may do.

Project configuration can bind reusable location rules to additional directories:

```yaml
access_paths:
  read: [/path/to/reference]
  write: [/path/to/scratch]
```

Add this mapping to `.shield/config.yml`, preserving its `schema_version`, `packs`
and any `rules` selection, then review the `manual` action in `shield policy review` and
confirm. An agent can also propose one directory at a time with the MCP
`access_path_add` change, which you confirm the same way. Editing a file alone
does not activate access. These settings are retained
when installing, updating or removing packs and when changing custom rules.

Rules with `{root: read}` use the project plus additional read and write roots;
`{root: write}` uses the project plus additional write roots. Write access includes
read access. At most 64 additional native absolute directories are accepted;
duplicates within each list, traversal components, NULs, null lists and unknown
fields refuse. Roots must be observable existing directories when evaluated.
An unavailable configured root makes its location predicate unknown.

Native Claude/Codex integrations also include system temporary roots automatically
in these unions; no `access_paths` entry is needed for ordinary temporary files.
Claude additionally supplies its verified current-session tool-result read scope.
These adapter observations are not stored in policy or accepted from diagnostic
tool input. Explicit policy restrictions and protected-file checks still apply.

Access paths bind policy predicates; the selected rules decide which operations
are permitted, including removal. They do not install a host filesystem sandbox.
Native hook cwd remains inside the enrolled project; absolute operands can refer
to additional roots. Protected Shield/operator configuration remains guarded.

## Action approvals

Use this workflow only for stored requests from the restricted SDK text-read
integration or bounded native runner. To handle an ordinary Claude or Codex
permission prompt, use the host's approval flow. To change future permissions,
review a policy change instead. See
[approval scopes](policy-decisions.md#approving-a-request-versus-changing-policy).

```sh
shield request list
shield request show REQUEST
shield request approve REQUEST
shield request deny REQUEST
```

These commands cover restricted text-read and checked native requests together.
Approval confirms one stored Ask request for an exact retry, with a five-minute
grant. Request resources, origin, project and active policy must still match.
Policy denials cannot be approved. Deny cancels a pending request or revokes an
unused grant. A consumed grant cannot authorize another execution.

Approval is available only through a controlling terminal. Redirected stdin,
JSON mode and intercepted agent tool calls do not supply operator confirmation.

## Request history

List requests, then copy an entry ID into `shield history show ENTRY`:

```sh
shield history list
shield history list --denied
shield history list --approved --agent claude
shield history show ENTRY
```

Entries combine request decisions, operator decisions, and reported completion.
Configuration changes are not request entries. Shell entries show the submitted
source: the list shows its first line, and `show` prints the full command and
deciding rule. File entries show the action and path, such as
`write /srv/acme/app.js`.

A decision alone does not establish execution. Completion without a reported
exit code stays without one; a blocked command is shown as not run. Rewrite
refusals appear as `ask → blocked (rewrite required)`, with the explanation
and suggested form in the detail view.

### Recording and retention

Initialization retains up to 5,000 requests. The history action in
`shield policy review` changes the limit to 1–10,000, disables recording, or clears
optional history. Clearing starts a new generation; increasing retention cannot
restore deleted records. Policy and authorization records are separate, so
clearing history neither removes active policy nor restores consumed grants.

History uses checksummed segments of at most 1 MiB, capped at 64 MiB per current
generation. Recovery files, retention rewrites, and readers holding old files
can use more storage. Segments reach stable storage on creation and when full;
a crash can lose buffered records from the current segment. Observations can
be visible before their receipt is recorded.

On rotation, segments containing only requests beyond retention are removed.
At the storage cap, the oldest segments are removed even if their requests
are within retention. The list then reports `evicted_history` and may contain
fewer entries than the configured limit.

### Completion links and missing data

Completions link only to a retained allowed request or Claude Ask handed to
host approval with matching operation, agent, session, tool-call ID, surface,
and cwd. Without a Shield reference, the complete host identity and operation
must identify exactly one request. Missing identity or reused IDs leave the
completion unlinked; matching command text alone is insufficient.

Linking completion to Ask preserves that decision and its prompt disposition.
It reports host execution, not individual operator approval. Shell exit status
stays `unknown` without structured exit evidence.

Displayed entries are limited to 8 KiB; `truncated` and `missing` identify omitted
details. Identical deliveries appear once. Changed content under one delivery
ID is marked `delivery_conflict`; torn writes are preserved as degraded history.
`status` reports unavailable optional history separately. Expired and terminal
authorization request state is reclaimed independently.
