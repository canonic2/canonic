# Troubleshoot Shield

Run `shield status` and `shield doctor` in the affected project. Follow any
reported recovery command, or find your symptom below.
In an agent session, ask the agent to use the `status` and `doctor` MCP tools
and follow their in-session recovery route. Cache relocation and native
integration reconciliation have their own confirmation forms; terminal commands
remain available when you choose to use them directly.

| What you observe | Where to look |
| --- | --- |
| No apparent Shield checks in the agent | [Integration loading](#the-agent-does-not-seem-to-use-shield) |
| Old behavior after installing a new Shield | [Reload the new version](#the-agent-still-uses-the-previous-shield) |
| A command proceeds despite a recorded Ask | [Codex Ask handling](#a-command-runs-even-though-policy-says-ask) |
| An unexpected Allow or Deny | [Inspect the request and matching rule](#the-request-was-allowed-or-blocked-unexpectedly) |
| A read-only GitHub GraphQL query is blocked | [Check versions, scope and query form](#a-read-only-github-graphql-query-is-blocked) |
| A YAML edit has no effect | [Review and activate the edit](#i-edited-a-rule-but-behavior-did-not-change) |
| An old proposal cannot be applied | [Stale proposals](#a-proposal-is-stale) |
| A pack proposal reports `candidate_error` | [Inconsistent pack candidate](#a-pack-proposal-reports-candidate_error) |
| Status reports repair or cache failure | [Policy files](#policy-files-need-repair) or [compiled cache](#the-compiled-policy-cache-cannot-load) |
| Initialization fails while downloading | [Baseline acquisition](#initialization-cannot-download-the-baseline) |
| Shield's project folder was moved or copied | [Confirm the new location](#the-project-was-moved-or-copied) |
| A change cannot obtain confirmation | [Terminal confirmation](#terminal-confirmation-is-unavailable) |

## A pack proposal reports `candidate_error`

Shield could not align a candidate's selected packs with its artifact bytes.
The proposal did not activate. Continue the task without that proposal and report
the error. Changing valid tool arguments or deleting project state is not a
recovery for this failure.

## The project was moved or copied

Ask the agent to call Shield's `status` tool. Shield can keep a restricted MCP
connection open when the saved enrollment names the old location, including
when the launch command still names that location. `project_enroll` shows a
confirmation form for the new folder and its captured policy. Accepting it
creates a fresh enrollment and continues on the same connection. Declining
leaves the saved state unchanged.
If the default compiled-cache location is unavailable, the agent can request a
fresh `project_enroll` form with an absolute `cache_dir`. The new location appears
in the review. This changes derived cache storage, not project permissions.

Moves and copies both require this review. Old approvals, session grants,
registry trust and agent registrations do not transfer. Shield preserves the
previous checkpoint and recent history as private recovery data. The agent can
then request any needed agent setup through its normal confirmation form.
Do not delete `.shield/review/` or edit project IDs to bypass enrollment.

`shield status` and `shield doctor` also explain a location mismatch without
changing state. If you choose to work directly in the terminal, `shield init`
offers the enrollment review there.
An explicit `--cache-dir` is included in that review.

## The agent does not seem to use Shield

Check whether the agent is registered with `shield status`. If it is missing,
run `shield mcp setup --agent claude` or `shield mcp setup --agent codex` in an interactive terminal,
review the changes, and confirm.

Restart the agent, trust the project, enable native hooks, and approve the MCP
connection. Codex also requires reviewing hook definitions through `/hooks`.
Use `shield doctor` to check for missing or changed owned files and binaries.

In JSON output, `enforcement` reports `installed`, `defined-for-box` (the Box
definition exists but no published hooks were found), `degraded`, or
`not-installed`. Policy-only inspection reports `not-checked` because it does not
inspect agent integration files. `enabled` separately reports whether Shield is
paused. For a Box installation, use `box config sync --server shield --dry-run`
to review what Box would publish.

Check `coverage.verification.agents` for recorded hook activity and its timestamp.
Recorded events show that callbacks ran at those times; they do not prove that
every current session loaded hooks. Missing or unavailable history also does not
prove that hooks are inactive. Use `shield history list` and
`shield history show ENTRY` to inspect the actual requests and outcomes.

Interrupted confirmed setup resumes on the next MCP request while preserving
unrelated settings. If an owned setting conflicts, the agent can request
`mcp_setup {"pending":"review"}` to show its replacement for your approval,
or `mcp_setup {"pending":"cancel"}` to review cancellation. Manual edits are
preserved unless you approve their replacement. The optional terminal routes
are `shield mcp setup --pending review` and `shield mcp setup --pending cancel`.

Hooks must be loaded to check requests. A host may continue execution if it
skips, disables, kills, or times out a hook. See
[loading and coverage](claude-project.md#load-the-hooks).

## The agent still uses the previous Shield

Hooks start the installed `shield` for every request, but the MCP server is one
process that the agent starts with its session. After installing a new build:

1. In each project, run `shield mcp setup --agent claude` or `shield mcp setup --agent codex` and
   confirm. Setup records the new executable's digest; until then
   `shield status` reports that the registered binary changed.
2. Exit the agent session and start a new one, so the agent launches the new MCP
   server. Reconnecting the MCP server from the agent (for example `/mcp` in
   Claude Code) also restarts it. An open session keeps the old server process.
3. Run `shield doctor`. If it reports the compiled cache as missing or
   incompatible, run `shield cache prepare`; the cache is rebuilt from the saved
   active policy without changing it.

Saved policy, proposals, history and approvals stay in `.shield/` across
updates. Do not delete `.shield/review/` to recover; only the compiled cache is
disposable.

## The request was allowed or blocked unexpectedly

Inspect the recorded request:

```sh
shield history list
```

Use an entry ID from that list in place of `ENTRY`:

```sh
shield history show ENTRY
shield rule list --json
```

Check the command, working directory, matching rule, failed requirements, and
active revision. Extra arguments or unknown options can stop a rule matching;
failed requirements can select its Ask or Deny branch.

Check the host action too: Codex's Allow fallback can let a policy Ask proceed.

For a blocked `rg` search, a message such as “Search operand `.agents` does not
exist” identifies the path Shield could not inspect. Remove an unintended missing
operand and retry the search with the intended existing paths. If the path is
needed, correct it first. Filesystem access failures, invalid directory components
and unresolved links have separate messages; they do not establish that a path is
absent. Correcting one operand does not approve other paths or unsupported options.

If the message asks for a rewrite, submit the supported literal form it names.
If it identifies an unsupported tool, a policy change cannot add that support.
Native Claude and Codex hooks defer unmodeled internal tools to host permissions;
the separate [read-only SDK adapter](claude-read-only.md) denies tools outside its
supported reads.
If policy returns Deny, ordinary approval cannot override it. Review a narrower
request or an appropriate policy change instead. See
[policy decisions](policy-decisions.md) for precedence and Ask handling.

## A read-only GitHub GraphQL query is blocked

Inspect `shield pack list`, `shield pack show PACK_SELECTOR` and the request's
`shield history show ENTRY` output. The published GitHub 0.3.0 pack asks for raw
API access when unscoped; its `cli-scope` precondition rejects API calls when the
optional `repository` setting is active. Read-only intent does not establish
permission, and updating the engine alone does not change installed pack rules.

Inspect the rule or precondition named in the recorded decision before proposing
an appropriate policy change. Adding `-R` does not prove the repository scope of
a GraphQL query. For custom rules using the implemented `graphql-v1` capability,
see the [GraphQL reference](rule-reference.md#graphql-requests) for supported
carriers, selection models and refusal conditions.

## A command runs even though policy says Ask

Check the Ask fallback with `shield status`. New Codex setups let valid Ask
requests proceed, including unmatched requests and Strict-profile Asks.
History still records Ask, without individual operator approval.

To reject an operation, review a matching Deny rule. Ask or a missing match
will not block it under the Allow fallback. See
[policy decisions](policy-decisions.md#what-happens-after-ask) and
[requirements with an unmet branch](policy-rules.md#match-commands-and-enforce-requirements).
The [integration guide](claude-project.md) covers exceptions and saved fallbacks.

## I edited a rule, but behavior did not change

Shield uses the reviewed active snapshot, not unreviewed file edits. Run:

```sh
shield policy review
```

Choose the `manual` action to review edited configuration or rules and confirm
the change. Use `shield status` to inspect the active revision afterwards.
Use `shield pack add` to acquire a new pack release rather than replacing its cached
artifact by hand.

For future custom-rule changes, `shield rule add`, `shield rule edit ID`, and
`shield rule remove ID` combine validation, review, and activation. See
[project policy](project-review.md#custom-rules-and-proposed-changes).

## A proposal is stale

Another policy or setup change can make a proposal stale. Ask the agent to
inspect current state and prepare a fresh proposal. Wait for each application
result before preparing the next; do not reuse old approval arguments. See
[proposal review](project-mcp.md#proposals-and-operator-review).

## Policy files need repair

If `shield status` reports projection repair, run `shield policy review` and choose
`repair`. This recreates missing generated policy files from the committed
snapshot. Other manual edits are preserved for manual review.

A reported committed change with repair needed can already be active even if a
projected file could not be written. Check status before retrying a mutation.
Writer ownership is released automatically after a crash. A live writer can
briefly cause a lock error; retry after its operation finishes. Shield preserves
legacy markers whose ownership cannot be verified and never removes a live
writer's lock because a wait timed out. See
[active policy and repair](runtime-activation.md).

## The compiled policy cache cannot load

Shield normally rebuilds missing, corrupt, or incompatible cache data from the
saved active policy. To prepare it explicitly, run:

```sh
shield cache prepare
```

This does not activate manual edits or unapplied reviews. If preparation still
fails, follow the reported cause: invalid retained source or an inaccessible
cache directory prevents evaluation. `shield status --json` reports the chosen
cache root. See [cache locations and overrides](compiled-cache.md).

## Initialization cannot download the baseline

Default initialization needs the pinned main registry. Check the reported
connection or verification error; failed acquisition leaves a new project
unenrolled. Retry `shield init` after resolving that cause.

If you intend to start without the baseline, use `shield init --no-baseline`.
This is a different policy choice, not a way to bypass artifact verification.
Unmatched requests return Ask. See [first setup](getting-started.md).

## Terminal confirmation is unavailable

Run the command yourself in an interactive, cursor-capable terminal. Redirected
input, JSON mode, and intercepted agent tool calls cannot provide CLI operator
confirmation. Enlarge the terminal if a prompt cannot fit.

Agents can prepare changes with `policy_propose` and request your host's
approval with `policy_apply`. If confirmation prompts are disabled, enable them
for the session, then prepare a fresh change if it is still needed. If the host
cannot present supported confirmation, the agent must close the attempt and
report that missing capability. Closed reviews remain inspectable through
`policy_receipt` and cannot be accepted later.
See [terminal controls](operator-experience.md#controls-and-output).
