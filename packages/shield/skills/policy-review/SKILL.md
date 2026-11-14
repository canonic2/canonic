---
name: policy-review
description: Prepare bounded session grants before repeated privileged operations, review exact approvals and recurring interruptions, and propose lasting policy changes through Shield's supported human approval routes.
---

# Review Shield permissions

Review permissions when requested or when `suggestion_list` marks recurring
groups eligible. Avoid reviews after every task or decision.

Keep approvals, connection changes and setup inside this conversation through
Shield MCP tools and the host's confirmation UI. Telling the user to run terminal
commands is a product bug, not an acceptable workaround. If a tool offers only
that route, report the missing in-session capability. For Codex with prompts
disabled, ask the user to enable them with `/permissions`, then prepare a fresh
attempt; never change host approval settings yourself.

Before a known batch of repeated privileged operations, inspect
`session_grant_show`. Use the native hook's `session_ref` to propose a bounded
temporary batch or constrained argv family with `session_grant_propose`; prefer
an exact batch when all requests are known. Copy `apply_arguments` unchanged
into `session_grant_approve` for the person's form. Do this before the batch,
without first attempting every command or adding a preliminary chat approval.
An ordinary grant covers Ask. Named Deny overrides require current owner-qualified
rule references, `branch: met`, and body digests from `rule_list`; failed
requirements, preconditions and protected state remain closed. A reference is
not approval. Respect the recorded target conversation when MCP association is
unavailable. Inspect existing offers before proposing, stop on decline, and
never broaden scope to avoid refusal. Use `session_grant_revoke` to end your
grant early.

Only when the person explicitly asks to reconsider a declined session grant,
call `session_grant_reconsider` with that declined `proposal_id`, its
`candidate_digest` and a fresh `idempotency_key`. It prepares the same scope,
duration and Deny overrides under a new candidate ID, preserving the decline.
Copy its new `apply_arguments` to `session_grant_approve` for a fresh human
form. A new proposal key alone cannot reopen a decline. Do not invoke
reconsideration automatically or change the scope to escape suppression.

1. For recurring interruptions, inspect `suggestion_list` and relevant `history_list`, `rule_list` or
   `policy_show` evidence. Follow returned next actions. Respect incomplete
   coverage, existing proposals and deferrals.
2. For one-time access, follow a returned `request_approve` route, wait for user
   approval, then repeat the identical call once. A refusal alone does not
   authorize a permanent permission change. Do not bypass unsupported operations.
3. For packs, inspect `pack_info` setup/settings. Select declared optional
   variable names needed for the requested restriction in `pack_add`'s `variables`; the
   form collects values. For example, AWS declares `profile`. Omit unrequested
   settings. Report missing scope support instead of proposing broader permissions.
   `local` means storage, not target scope.
   Use `packs` on `pack_add` for related selections and their variables.
4. For authorized custom rules, inspect command grammar and effects; preserve
   scope and deny boundaries. Use `policy_propose` schema examples and link
   returned review context. Batch related changes in `changes` (up to 16).
5. Prepare and immediately review the proposal; use `policy_show` if more evidence is needed. Copy
   `apply_arguments` unchanged into `policy_apply` through its returned approval
   route. Do not add a preliminary chat confirmation. Preparation and previews
   neither change the policy files nor authorize execution.
6. Stop on decline or cancellation. For uncertain completion, inspect current
   policy and the proposal with `policy_show` before retrying. Stale proposals need fresh
   preparation; unavailable approval follows the returned user route. Retry only
   while the task and authorization remain valid. Closed attempts cannot resume:
   prepare fresh with a new idempotency key. Never finish a task with an undecided
   proposal; explicitly withdraw anything abandoned. Cancellation, expiry,
   disconnect and task-end cleanup also close attempts automatically. Only when
   the user explicitly asks to defer, save intent with `policy_defer`. Drafts
   contain no candidate or approval; resume through fresh preparation and review.

Never edit protected state, run the operator CLI as an agent, change permission
modes, fabricate confirmation responses or impersonate another host adapter.
