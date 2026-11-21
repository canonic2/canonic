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
| A YAML edit has no effect or requests report invalid policy | [Check the policy files](#i-edited-a-rule-but-behavior-did-not-change) |
| An old proposal cannot be applied | [Stale proposals](#a-proposal-is-stale) |
| A pack proposal reports `candidate_error` | [Inconsistent pack candidate](#a-pack-proposal-reports-candidate_error) |
| Requests fail naming the compiled policy folder | [Compiled cache](#the-compiled-policy-cache-cannot-load) |
| Initialization fails while downloading | [Baseline acquisition](#initialization-cannot-download-the-baseline) |
| Shield's project folder was moved or copied | [Portable policy](#the-project-was-moved-or-copied) |
| A change cannot obtain confirmation | [Terminal confirmation](#terminal-confirmation-is-unavailable) |
| The hook says Codex cannot request approval | [In-session approval connection](#the-hook-says-codex-cannot-request-approval) |
| Shield says state is briefly locked | [Concurrent updates](#shield-says-state-is-briefly-locked) |

## Shield says state is briefly locked

Project authority and shared registry-trust acquisition wait briefly for an
active writer. The shared trust guard retries contention for up to 500 ms
before refusing; it checks current trust after acquiring the lock. Waiting
does not replay a request or consume a grant.

Another Shield operation held a required lock beyond this request's bounded
wait. The command has not been dispatched, so retry the same request after the
other operation finishes. This message is contention, not a policy Deny or a
rejected approval. Final authorization remains blocked until its required
state can be checked.

Repeated refusals during ordinary parallel requests need investigation. Use
[diagnostic logs](development.md#debug-builds-and-diagnostics) to distinguish
policy evaluation, final handoff and optional bookkeeping. Session-context
capture uses a short metadata transaction and does not prepare policy artifacts
under the writer lock.

## The hook says Codex cannot request approval

This message refers to the native command hook. A blocked approvable request
normally names `request_approve` and its request ID; Shield's MCP connection
then shows the human confirmation form. The native-hook message alone does not
mean an automatic reviewer rejected the request.

If the hook also says the approval connection is not registered, inspect
`shield status` in that exact checkout. An empty `registered_integrations` list
or an unowned MCP entry means the hook is checking requests without a registered
approval connection. Use the reported setup repair route and restart the agent
after publication. Keep unrelated hooks and MCP servers intact.

Hub and Box definitions in a fresh worktree are recognized on runtime startup.
Harmless YAML formatting and the known retired tool entries are compatible;
changed launchers, hooks, permissions or a foreign private executable pin remain
subject to review. See [Hub and Box setup](claude-project.md#set-up-through-hub-or-box).

## A pack proposal reports `candidate_error`

Shield could not align a candidate's selected packs with its artifact bytes.
The proposal changed no policy file. Continue the task without that proposal and report
the error. Changing valid tool arguments or deleting project state is not a
recovery for this failure.

## The project was moved or copied

A Shield project is its files. After a move, rename or copy, and in a worktree or
clone, Shield keeps working with no follow-up command: the files in `.shield/`
are the policy, so the next request is decided by them. A moved or copied folder
keeps its project ID, history, settings and pending in-session approval
requests. Pending text-read and checked-run requests are dropped; the agent
repeats the request. A new clone or worktree starts with fresh history and no
agent registration; run `shield mcp setup` there to connect an agent. A one-time
approval is used once, whichever folder uses it first.

A clone that lacks a pack release its lock file names fetches it from the
registry on `shield status`. Invalid files and missing packs prevent ordinary
policy authorization. Supported native requests can use exact in-session
review, with the problem in the message; fixed paths remain closed.
Corrupt state and unsupported formats are
separate problems with their own recovery.

New launch entries resolve `shield` from PATH and the project from the agent's
session folder. Restart an older agent to load the current binary. Do not delete
`.shield/state/` or edit project IDs after a move.

## The agent does not seem to use Shield

Check whether the agent is registered with `shield status`. If it is missing,
run `shield mcp setup --agent claude` or `shield mcp setup --agent codex` in an interactive terminal,
review the changes, and confirm.

Restart the agent, trust the project, enable native hooks, and approve the MCP
connection. Codex also requires reviewing hook definitions through `/hooks`.
Use `shield doctor` to check for missing or changed owned files and binaries.

In JSON output, `enforcement` reports `installed`, `defined-for-hub` or
`defined-for-box` (the Hub or Box definition exists but no published hooks were
found), `degraded`, or `not-installed`. `coverage.host_integration` describes
that definition, with `agent` naming `hub` or `box`. Policy-only inspection
reports `not-checked` because it does not inspect agent integration files.
`enabled` separately reports whether Shield is paused. For a Hub installation,
use `hub setup --dry-run` to review what Hub would publish; for Box, use
`box config sync --server shield --dry-run`.

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
3. Run `shield doctor` to confirm the integration. The new version compiles
   the policy files into a new plan on its first request.

Policy files, proposals, history and approvals stay in place across updates.
Do not delete `.shield/state/` to recover; only the compiled cache is
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

For `rg` searches, a missing file or directory within permitted locations can
proceed when the request meets the other policy requirements. Shield checks its
existing parent directories and links, and `rg` reports the missing path. For
example, `rg clientId src/missing.ts` can pass policy even when that file is absent.
The same checks apply to missing pattern files (`-f`) and `--ignore-file` paths.
Missing paths outside permitted roots or under exclusions still fail policy.
A missing search directory is also checked against excluded subtrees it could
cover.

For a blocked search, filesystem access failures, invalid directory components
and unresolved links have separate messages. Correct the named path or its access
before retrying. Correcting one operand does not approve other paths or
unsupported options.

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

Shield reads the policy files on every request, so a saved edit applies to the
next one. Run:

```sh
shield status
```

If status reports the files' error, fix the invalid files. A supported native
request can still use `request_approve` and an identical retry; supported Git
restorations can repair conflicted policy files through that review. Direct
policy edits, state and caches remain protected. See
[allowing one blocked request](project-review.md#allowing-one-blocked-request).
Otherwise inspect the request with
`shield history list` to see which rule decided it; another rule may take precedence.
Use `shield pack add` to acquire a new pack release rather than replacing its
artifact by hand.

`shield rule add`, `shield rule edit ID` and `shield rule remove ID` validate a
change and show it before writing. See
[policy files](policy-files.md) and
[project policy](project-review.md#custom-rules-and-proposed-changes).

## A proposal is stale

Another policy or setup change can make a proposal stale. Ask the agent to
inspect current state and prepare a fresh proposal. Wait for each application
result before preparing the next; do not reuse old approval arguments. See
[proposal review](project-mcp.md#proposals-and-operator-review).

## Another Shield operation is writing

A command that writes policy files holds a writer lock, released automatically
if Shield exits or crashes. A lock error means another operation is still
writing; retry after it finishes. If the files changed while a command was
showing its change, Shield refuses and asks you to run the command again.

## The compiled policy cache cannot load

Shield compiles missing, corrupt or mismatched plans from the policy files by
itself. A failure naming the compiled policy folder means Shield cannot use
the folder. Run `shield doctor`, then fix the folder's ownership or permissions,
or choose another with `shield mcp setup --cache-dir /absolute/path`. See
[compiled policy cache](compiled-cache.md).

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

On a host that supports in-session confirmation, agents can prepare changes
with `policy_propose` and request approval with `policy_apply`. If confirmation
prompts are disabled, enable them for the session, then prepare a fresh change
if it is still needed. If the host cannot present confirmation for a prepared
review, close that attempt. Use `policy_show` to inspect its outcome.

For a host without a supported approval adapter, Shield returns
`operator_review_required` before creating a proposal. The agent can use
inspection tools, report the missing capability, and continue with work the
active policy allows.
See [terminal controls](operator-experience.md#controls-and-output).
