# Understand policy decisions

Shield returns Allow, Ask, or Deny for a request. The agent host then handles
that decision using its integration and permission settings. For example,
Codex can let an Ask proceed, while its sandbox can still block an Allow.

## Packs, profiles, and custom rules

A permission pack contains reusable rules for a tool or workflow. Installing
one adds policy data, not the tool itself.

Each pack has a Loose, Recommended, or Strict profile. The author declares the
decisions; stricter profiles cannot be more permissive. Inspect those decisions
before choosing a profile.

Custom rules express project-specific choices, such as allowing one command or
requiring an argument. They can tighten or relax pack decisions, but cannot
bypass protected administration checks.

## What the three decisions mean

| Decision | Meaning | What to check next |
| --- | --- | --- |
| Allow | Policy permits the supported request in the supplied context. | The host's sandbox and other permissions can still apply. |
| Ask | Policy requires review, or no rule decides the request. | The host's approval capability and configured Ask handling determine the next step. |
| Deny | Policy rejects the request. | Change the request or review an appropriate policy change; ordinary approval cannot override Deny. |

Invalid policy, unavailable required state, and protection failures block rather
than becoming an unmatched Ask. Unsupported tools also have their own host
handling: Claude can ask for tool permission, while Codex blocks unsupported
tools. A broader rule cannot add support for a tool Shield cannot model.

## How rules combine

Consider a custom rule allowing only `acme-check verify`, where `acme-check` is
a fictional tool you have reviewed:

| Request | What to expect from that rule |
| --- | --- |
| `acme-check verify` | It matches and contributes Allow. |
| `acme-check verify --write` | It misses because the argument list changed. |
| `acme-check publish` | It misses because the operation changed. |

A missed rule contributes no decision. If no other rule decides a supported
request, Shield returns Ask. To reject a form, match it with Deny or an unmet
requirement that returns Deny.

Shield checks protected operations first, then project custom rules, then packs.

Within custom rules and within each pack, more specific matches win. An exact
argument list ranks above an unconstrained command rule. Ties resolve Deny,
then Ask, then Allow; file order does not matter.

Across packs, Deny wins over Ask, then Allow. Pack fallback rules yield to regular
rules from other packs. Matching custom rules take precedence over packs, so a
custom allowance can override a pack denial. Protected-state checks and denials
for other operations in the same request still apply.

For a compound shell request, an allowance for one command does not authorize
every other command or operation in it. See
[wrappers and shell commands](policy-rules.md#understand-wrappers-and-shell-commands)
for the analysis boundary.

## Conditions and requirements

Conditions decide whether a rule applies. Requirements decide which result an
applicable rule contributes.

Imagine a rule for a database command that requires `--read-only`. With that
option, the rule contributes Allow. Without it, an `on_unmet` branch can
contribute Deny and explain how to correct the request. If you put the option
only in conditions, a request without it misses the rule entirely; another rule
or the unmatched Ask fallback decides it.

This distinction matters when writing restrictions. Start with the
[rule-writing guide](policy-rules.md), which shows both matching and requirement
examples.

## What happens after Ask

Claude's supported native requests use its permission prompt or connected SDK
permission callback. Without a permission host, an unanswered Ask blocks.

Codex PreToolUse cannot show a Shield approval prompt. A new setup defaults its
Ask fallback to Allow for every profile; repeated setup preserves the saved
choice. With Allow, valid unmatched requests and explicit Ask rules can proceed.
The policy result remains Ask, and history does not record that as individual
operator approval. Codex's own sandbox and approvals still apply.

Some requests still block: protection failures, unsupported tools, requests
requiring a rewrite, and Asks fully covered by Shield's Codex prefix rules.
Automatic Ask allowance is also suspended while a confirmed setup needs repair.
See [native interception](claude-project.md) for the exact boundaries.

The separate `shield request approve` workflow grants one exact stored request for
supported SDK reads or checked native execution. It does not authorize a native
agent-hook retry. See [action approvals](project-review.md#action-approvals).

## Approving a request versus changing policy

| Your intent | Use | Effect |
| --- | --- | --- |
| Let a native agent request proceed after Ask | The host's permission flow, where supported | Handles that request under the host's approval rules. |
| Approve a stored SDK read or bounded native-run request | `shield request list`, then `shield request approve REQUEST` | Grants an exact retry under that workflow's limits. |
| Change how future matching requests are decided | A reviewed pack or custom-rule change | Publishes a new active policy. |

A policy proposal can change future permissions beyond the command that prompted
it. Review its matching scope before accepting. See
[proposal review](project-mcp.md#proposals-and-operator-review).

## Inspect a decision

Use `shield history list`, then `shield history show ENTRY` with an ID from the list,
to see a recorded request and the rule that decided it. Use `shield rule list --json`
to inspect active rules and their owners.

An agent can use `policy_evaluate` through [Project MCP](project-mcp.md#diagnostic-input-and-snapshots)
to evaluate a diagnostic request. Diagnostics do not execute commands, approve
them, or establish live enforcement. Native hooks evaluate the submitted command
and available context; they do not attest to every effect of execution.

For unexpected results, follow [troubleshooting](troubleshooting.md#the-request-was-allowed-or-blocked-unexpectedly).
