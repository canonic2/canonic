# Troubleshoot Shield

Run `shield status` and `shield doctor` in the affected project. Follow any
reported recovery command, or find your symptom below.

| What you observe | Where to look |
| --- | --- |
| No apparent Shield checks in the agent | [Integration loading](#the-agent-does-not-seem-to-use-shield) |
| Old behavior after installing a new Shield | [Reload the new version](#the-agent-still-uses-the-previous-shield) |
| A command proceeds despite a recorded Ask | [Codex Ask handling](#a-command-runs-even-though-policy-says-ask) |
| An unexpected Allow or Deny | [Inspect the request and matching rule](#the-request-was-allowed-or-blocked-unexpectedly) |
| A YAML edit has no effect | [Review and activate the edit](#i-edited-a-rule-but-behavior-did-not-change) |
| An old proposal cannot be applied | [Stale proposals](#a-proposal-is-stale) |
| Status reports repair or cache failure | [Policy files](#policy-files-need-repair) or [compiled cache](#the-compiled-policy-cache-cannot-load) |
| Initialization fails while downloading | [Baseline acquisition](#initialization-cannot-download-the-baseline) |
| A change cannot obtain confirmation | [Terminal confirmation](#terminal-confirmation-is-unavailable) |

## The agent does not seem to use Shield

Check whether the agent is registered with `shield status`. If it is missing,
run `shield mcp setup --agent claude` or `shield mcp setup --agent codex` in an interactive terminal,
review the changes, and confirm.

Restart the agent, trust the project, enable native hooks, and approve the MCP
connection. Codex also requires reviewing hook definitions through `/hooks`.
Use `shield doctor` to check for missing or changed owned files and binaries.

If doctor reports an interrupted confirmed setup, use `shield mcp setup` to
finish it. If an owned entry was edited, inspect the named configuration and
resolve the conflict; Shield preserves conflicting edits instead of replacing
them silently.

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

If the message asks for a rewrite, submit the supported literal form it names.
If it identifies an unsupported tool, a policy change cannot add that support.
If policy returns Deny, ordinary approval cannot override it. Review a narrower
request or an appropriate policy change instead. See
[policy decisions](policy-decisions.md) for precedence and Ask handling.

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
For lock errors, preserve the private review state and inspect the diagnostic;
Shield does not automatically steal an abandoned writer lock. See
[active policy and repair](runtime-activation.md).

## The compiled policy cache cannot load

Shield normally rebuilds missing, corrupt, or incompatible cache data from the
saved active policy. To prepare it explicitly, run:

```sh
shield cache prepare
```

This does not activate manual edits or pending proposals. If preparation still
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
approval with `policy_apply`. If the host cannot complete that approval,
review pending proposals yourself with `shield policy review`.
See [terminal controls](operator-experience.md#controls-and-output).
