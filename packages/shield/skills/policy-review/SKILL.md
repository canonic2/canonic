---
name: policy-review
description: Review recurring Shield permission interruptions when the user asks, interpret retained evidence, and prepare a narrow lasting policy change through Shield's supported approval route.
---

# Review Shield permissions

Review permissions when requested or when `suggestion_list` marks recurring
groups eligible. Avoid reviews after every task or decision.

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
   Read bundled guides only for missing rule syntax, not undeclared pack settings.
   For unreviewed file edits, propose `{"change":{"kind":"project_files_adopt"}}` alone, obtain approval,
   then prepare the original change. Changed files require fresh adoption.
5. Review the proposal; use `policy_show` if more evidence is needed. Copy
   `apply_arguments` unchanged into `policy_apply` through its returned approval
   route. Do not add a preliminary chat confirmation. Preparation and previews
   neither activate policy nor authorize execution.
6. Stop on decline or cancellation. For uncertain completion, inspect current
   policy and `policy_receipt` before retrying. Stale proposals need fresh
   preparation; unavailable approval follows the returned user route. Retry only
   while the task and authorization remain valid.

Never edit protected state, run the operator CLI as an agent, change permission
modes, fabricate confirmation responses or impersonate another host adapter.
