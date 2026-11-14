# Project policy, approvals and history

Use this guide to manage an initialized project: inspect policy, connect agents,
review changes, and investigate requests. If this is your first setup, follow
[your first project](getting-started.md) first. For the meaning of Allow, Ask,
and Deny, read [policy decisions](policy-decisions.md).

## Choose a task

| You want to… | Start with |
| --- | --- |
| See what is active | `shield status`; use `shield doctor` for a problem. |
| Pause or resume native checks | [Pause or resume Shield](#pause-or-resume-shield) |
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
checks the policy files, registry checkpoints, pack artifacts, and the
binary your agents run, and gives repair commands when needed. JSON output
provides full structured details for scripts and agents.

Status and doctor can inspect a validated historical checkpoint without saving
its conversion. `checkpoint_upgrade_pending: true` reports that the upgrade is
still unsaved. If the review directory is read-only or the disk is full, restore
write access or free space before retrying the operation. Hooks and changes
require the conversion to be saved before proceeding.

Doctor distinguishes agent integration repair (`shield mcp setup`) from
problems in the policy files, which `policy_files.error` names; fix those in
the files. Follow the command for the reported problem.

## Pause or resume Shield

```sh
shield disable
shield enable
shield status
```

Each change shows a review and requires confirmation in your terminal. Agents
can request the same change with `disable {}` or `enable {}`;
Shield shows you a confirmation form and changes the setting only if you accept.
JSON mode and redirected input cannot confirm it.

The command writes `enforcement: paused` or `enabled` to your uncommitted
`.shield/config.local.yml`, so other developers keep their own setting; a value
committed in `config.yml` applies to everyone who has not set their own. See
[settings](policy-files.md#settings). Policy, packs, history, grants and agent
registrations remain in place. `status` and `doctor` report whether
Shield is enabled.

While disabled, native hooks retain protected-file and operator-command guards.
Agents cannot use the pause to edit runtime state, forge grants or invoke the
operator CLI. Saved protected-file Allow choices remain in force; other
protected accesses are blocked during the pause. Shell requests
must have a verifiable protected footprint; unsupported or incomplete analysis
is blocked. Other requests return no permission decision and follow your
agent's own permissions and sandbox. Project rules are not applied, and
Shield records no native request history during this pause.
Its MCP tools remain available, with human confirmation still required for
administrative changes. Explicit checked runs and SDK consumers continue to
enforce policy.

Hooks inspect submitted requests; they do not isolate same-user processes or
authenticate edits made outside interception. The enable review makes explicit
that resuming applies the policy files as they are, including any edits made
while paused.

Disabling removes Shield-generated Codex permission rules; enabling restores
them from the policy files. Restart connected agents when the result requests it,
so they refresh cached rules. Native hooks read the setting on their next
invocation; a toggle does not cancel requests already underway. Enabling
requires readable policy files. If restoring permission rules needs repair,
checks are already enabled and the result names `shield mcp setup`.

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
there is no Block picker or flag. Deny, protected files, and invalid mapped requests
still block. Unmodeled internal tools defer to Codex permissions. Codex's own
sandbox and approvals also apply.

Status and doctor report the effective Ask fallback. Missing authorization or
an interrupted setup can report Block; finish the plan with `shield mcp setup`.
See [Ask handling](policy-decisions.md#what-happens-after-ask) and the
[integration guide](claude-project.md) for loading and coverage.

## Protected files

```sh
shield protection set
```

When policy checks are enabled, native hooks ask by default before agents read
`.mcp.json` and agent hook settings (`.claude/settings.local.json`,
`.claude/settings.json`, `.codex/hooks.json`, `.codex/config.toml`),
which may hold credentials, and block agents from changing them. Agents can never
change the `.shield` policy files, whatever you choose: those files are the
policy, so agents change it by proposal.

The same settings protection covers the adapter's configured agent home,
including a relocated home. Verified session-output reads retain their separate
scope; they do not permit configuration writes.

`protection set` asks about each group and confirms the result:

| Group | Default | Other choice |
| --- | --- | --- |
| Reading agent settings | Ask, or block where the agent cannot prompt | Allow without asking |
| Changing agent settings | Block | Allow; the agent can then remove Shield's hooks or the host confirmation that MCP policy changes rely on |

Reading agent settings includes commands whose output can print them without
naming them, such as `git show HEAD:.mcp.json`, an unscoped `git diff` or
`git log -p`, `git grep`, and `grep -r` over a folder that holds them. Add a
pathspec that leaves settings out, as in `git diff -- src`, to keep such a
command out of this group. Listing names and metadata, as `ls`, `find`,
`git ls-files` and `git status` do, is not reading; `git ls-files --stage`,
`git status --porcelain=v2` and `git status -v` are, because they print object IDs
or changes. The same footprint check applies when those commands omit paths.
Unknown Git subcommands can be aliases or external commands, so their output
conservatively reaches repository settings. Explicit `-c` or `--config-env`
overrides of `alias.*`, `core.pager` or `diff.external` use that same footprint.
Git option values do not narrow this footprint: in
`git diff --ignore-matching-lines src`, `src` is a regular expression, so the
diff can include settings outside that folder. Unknown option arity also keeps
the complete footprint rather than guessing which words are paths.

`.shield/state/`, compiled caches, the `.shield` folder itself and
`.shield/.gitignore` stay protected whatever you choose, and agents still cannot
run Shield operator actions. Policy files are never writable by agents, in
`.shield/`, `.config/shield/` or a `--config` file's folder. Help-only requests
follow ordinary policy. The choice is the `protection` setting, written to your
`config.local.yml`
([settings](policy-files.md#settings)); agents cannot change it. `status` lists
any group you have opened. An invalid `protection` value makes every native
request ask until you fix it or run `protection set` again.

### Allowing one blocked request

You don't need to change these settings to let an agent do one blocked thing.
When Shield blocks a change to agent settings or policy files, a credentials
read the agent cannot ask about, a policy Deny, a Codex request that needs
approval, or a Codex tool Shield cannot check, the agent can ask you in the
same conversation. It calls Shield's `request_approve` tool, and your agent
shows Shield's form with the exact action, the folder, why Shield blocked it
and what allowing it can do. If you accept, the agent repeats the identical
request once within five minutes; anything else is still blocked, and your
policy and protection settings do not change. If you decline, the agent
continues without it.

When one task needs several blocked requests, such as deleting the same branch
in two repositories, the agent can ask about up to eight of them in one form,
subject to a 12,000-byte limit for the complete confirmation. It should include
only requests still needed for the task. If a group is too long, Shield approves
nothing and returns `retry_arguments` for a prefix that fits, plus
`remaining_request_ids` to review separately. Each new call still needs your
confirmation. The agent can also ask about just one request, for example
`request_approve({"request_id":"hook-request-acme-1"})`, using an ID from its refusal.
The form starts by saying how many one-time permissions accepting creates and
how many override a deny or change agent settings, then shows each request in
full. Accepting allows each listed request once; the agent repeats each one
identically within five minutes, in any order. If any request in the group can
no longer be allowed, for example because it expired or your policy changed
while the form was open, nothing is allowed.

Codex shows the form only when its approval policy allows prompts. If the
agent reports that it could not ask you, choose an approval policy that allows
prompts with `/permissions` in the same Codex session; the agent then asks
again.

Runtime state, caches, the `.shield` folder and its ignore file are never
offered, even once, and neither is a request that also touches them.

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
Confirmation writes the validated change to the policy files; cancellation leaves
them unchanged.

### Review an agent's proposal

Agents can propose packs, rules, domain permissions, or folder access. Check
whether a proposal covers one invocation or a broader set of requests. The agent
prepares the exact change and immediately requests approval in the conversation.
Accept applies it; decline rejects it; cancellation, timeout, disconnection or
task completion closes an unapplied attempt. Changes to its base make it stale.
Closed attempts cannot be accepted later.

For a direct operator review of a live change, run:

```sh
shield policy review
```

Choose the live change explicitly. The review shows the change, agent's reason,
originating command, and permission comparison. Tab opens the complete change,
including pack rules. Choose Accept, Reject, or Cancel review; cancellation
withdraws that attempt. There is no oldest-first approval queue.

Agents can also submit a proposal's exact `apply_arguments` to
`policy_apply` for host approval. `policy_show` reads a proposal's outcome.
Deferred work is saved as intent only when you request it, and needs fresh
preparation and approval when resumed.
See [MCP review](project-mcp.md#proposals-and-operator-review).
A proposal builds on the policy files as they are, including your hand edits,
and keeps them when applied.

Changes to the policy files, setup or registry settings can invalidate
a proposal. Ask the agent to prepare a fresh one, even if the policy revision
has not changed. Independently confirmed protected-file settings remain current.
For successive proposals, wait for each result before preparing the next.

### Edit the policy files by hand

The policy files are the policy. An edit to configuration or rules applies to
the next request; inspect `shield rule list --json` for the result, and
`shield status` for an error if the edit made the files invalid. Use
`shield pack add` or `shield pack update` to acquire releases rather than
editing pack artifacts. See [policy files and settings](policy-files.md).

When Shield writes a change, it preserves comments, blank lines, and layout outside changed entries.
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
and any `rules` selection; the next request uses it. An agent can also propose
one directory at a time with the MCP `access_path_add` change, which you
confirm in the host. These settings are retained
when installing, updating or removing packs and when changing custom rules.

Rules with `{root: read}` use the project plus additional read and write roots;
`{root: write}` uses the project plus additional write roots. Write access includes
read access. At most 64 additional native absolute directories are accepted;
duplicates within each list, traversal components, NULs, null lists and unknown
fields refuse. A configured root that does not exist when evaluated contains
nothing, so it leaves the union and the remaining roots still decide. Exclusions
retain missing roots, protecting entries created through a containing root.
A root that exists but is not a directory, or cannot be inspected, makes its
location predicate unknown, and requests that depend on it are blocked.

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

Read, checked-run and native-hook terminal outcomes remain available for up to ten minutes
within a separate bounded receipt store. Repeating the original decision returns
the closed outcome without issuing permission. All-terminal groups can reconcile
without another form; approve only the live IDs when a group mixes live and closed
requests. Older or evicted IDs report unavailable.
For native hooks, reporting a consumed outcome leaves its separate one-use
follow-up callback available for the original 60-second window. Reporting never
renews that window or authorizes another execution. Unreadable optional outcome
metadata stays preserved locally and unavailable; it does not prevent valid policy
or live grants from loading.

Fresh approval requires a controlling terminal or the registered host's Shield
confirmation form. Redirected stdin, JSON flags and intercepted tool input do not
supply operator confirmation.

## Request history

List requests, then copy an entry ID into `shield history show ENTRY`:

```sh
shield history list
shield history list --denied
shield history list --approved --agent claude
shield history show ENTRY
```

Entries combine request decisions, operator decisions, and reported completion.
Policy changes are recorded by the policy files themselves; use your version
control to see how they changed. `shield policy show` lists recent proposal
outcomes.

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

In an agent session, `history_configure` offers the same controls through a human
form. If the optional history lock's permissions have drifted, confirming a
control restores private permissions on the existing owned lock before applying
it. Declining changes neither permissions nor settings. Shield waits for a live
history writer and refuses linked, foreign or replaced lock files; it never
deletes a lock to force a control through. Changing retention still requires
valid journal content. Use the confirmed clear control to reset corrupt optional
history while preserving policy and grants.

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
