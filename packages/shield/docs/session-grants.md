# Approve a batch for one conversation

Session grants let you approve repeated operations during one Claude Code or
Codex conversation. Your project's packs and policy stay unchanged. Other
conversations keep their normal Ask and Deny decisions.

For example, while reviewing dependency PRs you can approve a fixed set of
squash merges, or a constrained merge family when PR numbers will be discovered
during the task. Prefer the fixed set when the batch is already known.

## Agent workflow

Before a known batch of privileged commands, inspect `session_grant_show`.
Shield's native hook supplies a `session_ref` after an ordinary shell request;
this reference selects a recorded conversation, and grants no permission.

Call `session_grant_propose` with that reference, an `idempotency_key`, and a
`scope` array. Every array entry is an alternative; all constraints within an
entry must match. Entries use the normal argv rule `conditions` and optional
`requirements`. Supported entries require literal executable names, a project
cwd constraint, and exact arguments or a closed options grammar. Unknown
options, opaque shell requests, and non-shell tools cannot use a grant.
Requests whose policy analysis remaps argv to another directory or a file
consumer also retain their ordinary approval route.

This example permits two exact commands from the project root:

```json
{
  "session_ref": "COPY_THE_NATIVE_SESSION_REF",
  "idempotency_key": "dependency-merges-1",
  "duration_seconds": 7200,
  "reason": "Merge the two dependency PRs after completing their reviews.",
  "scope": [
    {
      "conditions": {
        "domain": "argv",
        "executable": "gh",
        "arguments": {
          "kind": "exact",
          "values": ["pr", "merge", "42", "--repo", "acme/app", "--squash"]
        },
        "cwd": {"exact": {"root": "project"}}
      }
    },
    {
      "conditions": {
        "domain": "argv",
        "executable": "gh",
        "arguments": {
          "kind": "exact",
          "values": ["pr", "merge", "43", "--repo", "acme/app", "--squash"]
        },
        "cwd": {"exact": {"root": "project"}}
      }
    }
  ]
}
```

Copy the returned `apply_arguments` unchanged to `session_grant_approve`. Shield
shows its confirmation form to the person. Accepting a tool-call permission,
a chat message, or an automatic command review does not accept this form.
If forms are unavailable, the person can review the candidate in their terminal.
The agent must not run the operator approval command itself.

After acceptance, submit covered native commands normally. No grant ID is added
to commands. Shield checks the recorded host conversation, current policy and
protection state, expiry, every covered operation, and the scope on each use.
It records the original policy decision separately from grant authorization.

Repeat proposals and accepted apply calls reuse the retained result; they do
not extend its expiry. Equivalent pending, active or declined offers return the
existing record. Do not repeatedly offer declined permission or broaden the
scope to work around a refusal.

## Reconsider a declined offer

If the person explicitly asks to reconsider, inspect the declined record with
`session_grant_show`. Call `session_grant_reconsider` using its `proposal_id`,
`candidate_digest` and a fresh `idempotency_key`:

```json
{
  "proposal_id": "COPY_THE_DECLINED_PROPOSAL_ID",
  "candidate_digest": "COPY_ITS_CANDIDATE_DIGEST",
  "idempotency_key": "explicit-reconsideration-1"
}
```

This prepares a new candidate with the same recorded conversation, scope,
duration, reason and Deny overrides. It preserves the original decline and grants
no permission. Copy the new candidate's `apply_arguments` to
`session_grant_approve`. The fresh form identifies the earlier decline and shows
the complete permission again. Accept issues the new grant; Decline or Cancel
leaves both decisions recorded without permission.

Do not call reconsideration automatically. Changing a normal proposal's retry
key alone keeps the decline suppressed. Repeating the same reconsideration key
returns its retained result; another key cannot duplicate a matching pending
review or renew a live grant. A fresh decline stays suppressed too. Changed
policy, protection, registration or an unavailable native context requires
inspection and a current proposal, rather than reviving the old candidate.

## What the person approves

The review identifies the project, agent and recorded conversation, the complete
scope, lifetime, and any explicitly named Deny overrides. Shared MCP servers may
have no trusted link to the current conversation; the form states this and
explicitly targets the recorded conversation shown.

The default lifetime is two hours, with a minimum of one minute and maximum of
eight hours. Matching uses are unlimited during that period. Resuming the same
conversation can reuse an unexpired grant. A new conversation cannot. Process
exit is not evidence that a conversation ended.

Revocation, changed protection settings, and changed native
registration invalidate the permission. Clock discontinuities also make it
unavailable; obtain fresh confirmation. A system restart or suspend may trigger
this conservative clock check on some platforms.

An ordinary grant covers Ask decisions. To override a policy Deny, the agent
must select each current rule through `rule_list` and include its `rule_ref`,
`branch: "met"`, and `rule_digest` in `override_denies`. Shield prominently shows
these exceptions. Failed requirements, preconditions, fixed protected state and
unresolved operations cannot be overridden this way.

Permission does not prove that review happened, remote checks passed, or a
command succeeded. Continue checking those task requirements before execution.

## Inspect and revoke

```sh
shield session-grant list
shield session-grant show PROPOSAL_OR_GRANT_ID
shield session-grant propose candidate.json
shield session-grant approve PROPOSAL_ID
shield session-grant revoke PROPOSAL_OR_GRANT_ID
shield history list --authorization-kind session_grant
shield history list --grant-id GRANT_ID
```

Approval and operator revocation require the controlling terminal. JSON mode is
read-only. Agents inspect through `session_grant_show` and can reduce their own
authority through `session_grant_revoke`; another connection's grant may require
operator revocation. History is optional and does not own live permission.

List inspection accepts `session_ref`, `cursor` and `limit` (1–64, default 16).
Selecting a `proposal_id` or `grant_id` returns one record and cannot be combined
with paging. Restart pagination when state changes and its cursor becomes stale.
Terminal receipts remain available for up to 24 hours before reclamation.

Existing installations retain saved MCP tool permissions. After installing a
build with these tools, rerun `shield mcp setup` for the selected agent, review
its tool permissions, and restart the agent so it loads the new catalog.
