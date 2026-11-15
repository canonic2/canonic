---
name: policy-audit
description: Audit and clean up a project's Shield policy by finding stale, unused or redundant custom rules, dead-weight packs and pack overrides, and custom rules that allow concrete harm; propose removing or narrowing them for the user to approve.
---

# Audit the project's Shield policy

Use when the user asks to audit, clean up, prune or review this project's Shield
policy. The audit covers custom rules, pack overrides and whether each installed
pack is still needed. It does not judge the rules inside packs. Nothing changes
until the user approves the proposal.

## 1. Read the policy

- `rule_list` for every custom rule, with its layer, decision and any
  `expires_at`.
- `pack_list` for installed packs and the dependency edges between them.
- `pack_show` for each pack with overrides, to see what each override changes
  from the pack's own decision.

## 2. Gather evidence

From the repository, for each custom rule, find what it names (Make target,
package script, package folder, executable) and read what that runs today,
following the scripts and tools it calls. For each pack, check whether the
repository still uses its executables.

From history, page through recent `history_list` entries and note which rules
and packs decided requests. When an entry does not name the deciding rule,
which is the case for native shell commands, `policy_evaluate` the recorded
request to find the rule that decides it now. History covers only this machine
and its retention, so recent use is evidence a rule is needed; no use is not
evidence by itself.

## 3. Classify

Flag only these. Everything else stays.

- **Dead rule:** `expires_at` has passed, or what it names is gone from the
  repository and history shows no recent use.
- **Redundant rule:** an installed pack decides its commands the same way
  without it. Confirm in step 5.
- **Dead pack:** the repository no longer uses its executables, history shows no
  recent decisions from it, and no other installed pack depends on it. Keep
  general packs that are not tied to one stack.
- **Override to clear:** it sets the decision the pack already has, or it
  loosens a pack decision and fails the danger test below.
- **Dangerous rule:** an allow for which you can name a concrete harmful command
  that `policy_evaluate` shows it allows. Ask and deny rules only restrict, so
  they are never dangerous. Look for:
  - an allow that overrides a pack's deny, where the deny's reason still applies;
  - a wrapper whose target now runs more than development work, such as a
    deploy, publish or push;
  - any-argument or prefix matches on interpreters and script runners, which
    run arbitrary code;
  - working folders, file writes or fetches that reach outside the project.

A broad allow for harmless work, such as formatting or lint, is not a finding.
Without a concrete harmful command, there is no finding.

For each dangerous rule, choose the fix from the concern: narrow it with
`rule_edit` when it serves real development work, or remove it when nothing
legitimate remains. A narrowed rule keeps its `id` and matches the commands as
agents run them, with `exact` arguments or `options` listing only reviewed
flags and values, never a `prefix` for a script runner, and a project `cwd`.

## 4. Propose

1. Tell the user the findings, one line each: what, why, and for a dangerous
   rule the example command. Group them as remove and narrow.
2. Call `policy_propose` with all fixes in `changes`: `rule_remove`,
   `rule_edit`, `pack_remove`, and `pack_configure` with `clear_override` for
   overrides. Name the evidence in `agent_reason`. A proposal holds up to 16
   changes; apply each before preparing the next.

## 5. Check before applying

`policy_evaluate` against the proposal, passing its `after_candidate_id` as
`candidate_id`:

- A removed redundant rule's commands still get the same decision.
- Each dangerous example command is no longer allowed.
- A narrowed rule still allows the forms it is meant for.
- Commands recently used in history still get the same decision, unless
  changing them was the point.

If any check fails, close the proposal with `policy_withdraw`, drop or correct
the affected change and propose again.

## 6. Apply and finish

- Copy `apply_arguments` unchanged to `policy_apply`. Its form is the approval;
  do not also ask in chat.
- On decline or cancellation, stop; do not propose the same changes again.
- On `stale_revision`, read the policy again and prepare a new proposal.
- Do not finish with an undecided proposal; close an abandoned one with
  `policy_withdraw`.
- Report in a few lines what was removed and narrowed, and what you kept despite
  looking unused, with the reason.
