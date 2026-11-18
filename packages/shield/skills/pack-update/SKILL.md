---
name: pack-update
description: Update a project's installed Shield packs to their latest compatible releases by refreshing release metadata, preparing one update proposal, explaining the version and permission changes, and submitting it for the user's approval.
---

# Update installed packs

Use when the user asks to update Shield packs, bring them to the latest
release, or act on a pack update notice. A notice alone is not a request:
mention it after the current task, and prepare an update only when the user
asks. Nothing changes until the user approves the proposal.

## 1. Read what is installed

`pack_list` lists each installed selection with its selector, version, profile,
tracking or pinned mode and dependency edges. An explicit update moves selected
packs, including pins, to their newest compatible release. Profiles and pin mode
are preserved at the resulting versions. Dependencies follow their parents'
constraints. Nothing updates automatically.

## 2. Refresh release metadata

Availability in `pack_updates` comes from cached catalog metadata, so it can be
stale. Refresh it before deciding anything:

- `registry_list` names the sources installed packs come from.
- `pack_search` with `{"query":"main/"}` refreshes the main registry; use
  each other source's alias the same way.
- `pack_list` again shows `pack_updates` from the fresh metadata.

`availability: unknown` or stale evidence means the refresh did not happen or
failed. Never report packs as current from it.

## 3. Prepare one update

Call `pack_update` with `{}` for every direct selection, including pins, or with
`{"reference":"<selector>"}` for one selection and its dependencies.

- If a release adds settings, Shield asks the user in a form. Do not guess
  values or fill them in yourself.
- `unchanged` means no newer compatible release exists. Report it and stop.
- For an exact release, use `pack_update` with
  `{"reference":"<publisher>/<pack>@<version>"}`. Updating a pin requires no
  reinstall or separate unpin step.
- Compare each proposed version with the newest advertised one in
  `pack_updates`. When the proposal stops short, the newer release needs a
  newer Shield, other settings or compatible dependencies. `pack_info` on that
  release reports its `compatibility`: the Shield `engines` range it needs and
  whether this Shield supports it. Report what holds it back rather than
  forcing it.

## 4. Explain the change

Tell the user, one line per pack, the version change and that the profile is
unchanged. Then check what the update changes for this project's real work:

- From `history_list`, take recent requests that asked or were denied, and the
  project's everyday commands.
- `policy_evaluate` each one twice: once as policy stands, and once with the
  proposal's `after_candidate_id` as `candidate_id`.
- Report the decisions that change, such as "`gh pr view BRANCH -R acme/web`
  now allows". If a command the project relies on would become denied, say so
  and let the user decide before approving.

## 5. Apply

- Copy `apply_arguments` unchanged to `policy_apply`. Its form is the approval;
  do not also ask in chat.
- On decline or cancellation, stop. Do not prepare the same update again.
- On `stale_revision`, read `pack_list` again and prepare a new update.
- Do not finish with an undecided proposal; close an abandoned one with
  `policy_withdraw`.

## 6. Finish

Report in a few lines:

- each pack's old and new version;
- pin modes retained, and releases held back with the reason;
- the decisions that changed.

Custom rules written to work around a gap the update closed may now be
redundant. Mention it, and leave removing them to a separate request.
