---
name: pack-update
description: Prepare and confirm a requested update to selected Shield packs, or check which updates are available.
---

# Update installed packs

When the user asks to update, call `pack_update` directly. Mention a pack
update notice after the current task. Prepare an update only on request.

## Prepare the requested update

- For one pack, pass `{"reference":"<selector>"}`. Use `pack_list` if the
  installed selector is unclear.
- For an exact release, call `pack_update` with
  `{"reference":"<publisher>/<pack>@<version>"}`.
- For all direct packs, including pins, pass `{}`.

Shield fetches the catalog metadata it needs, resolves compatible dependencies,
and preserves the profile and pin mode at the resulting versions. If a release
needs settings, let the user fill in Shield's form. If it returns `unchanged`,
report that and stop. When a known newer release is held back, use `pack_info`
to check compatibility and report the blocker.

## Review and apply

Report each version change and any held-back release. Check the proposal's
permission changes against the project's relevant work:

- From `history_list`, take recent requests that asked or were denied, and the
  project's everyday commands.
- `policy_evaluate` each one twice: once as policy stands, and once with the
  proposal's `after_candidate_id` as `candidate_id`.
- Report the decisions that change, such as "`gh pr view BRANCH -R acme/web`
  now allows". If a command the project relies on would become denied, say so
  and let the user decide before approving.

- Copy `apply_arguments` unchanged to `policy_apply`. Its form is the approval;
  do not also ask in chat.
- On decline or cancellation, stop. Do not prepare the same update again.
- On `stale_revision`, read `pack_list` again and prepare a new update.
- Do not finish with an undecided proposal; close an abandoned one with
  `policy_withdraw`.

## Check availability without preparing an update

Read `pack_list` and its cached `pack_updates`. Refresh stale or missing
metadata with `pack_search` using the relevant alias, such as
`{"query":"main/"}`, then read `pack_updates` again. Use `registry_list` if
the source is unclear. Pinned direct selections participate. Zero
`direct_total` means there were no direct selections to check. Report partial,
unknown or stale availability as such.
