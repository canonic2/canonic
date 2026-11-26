---
name: project-setup
description: Initialize Shield, connect native agents or Hub publication, enable enforcement and verify setup. Configure development packs and project rules when requested.
---

# Configure Shield for this project

Use when the user asks to set up, configure or refresh Shield for this project.
Complete the requested setup through the tools in this session, preserving
existing policy, packs, settings and agent selections.

## Initialize and connect

1. Call `setup` on a fresh or enrolled connection. Omit `agents` to reuse saved
   choices or detect the publisher or native agent. Ask the user to choose when
   discovery is ambiguous. The baseline defaults to Recommended on first use.
   Let the user confirm material changes. If the host cannot show the review,
   ask the user to run the returned operator command.
2. Follow the returned `next` action and its arguments. For a Hub project, call
   Hub's `setup` with the returned project and preserve its saved agent selection.
   Hub's own review confirms publication of the Shield definition and hooks.
   Use Hub's setup preview when the user wants to inspect that plan first. If the
   host cannot show Hub's review, follow Hub's scoped operator action. If Hub
   reports missing owned hooks, use its `error.context.next` for the adoption
   review.
   An interrupted Shield publication resumes when
   `setup` is called again without arguments.
3. If enforcement is disabled and setup was requested, call `enable` for the
   user's in-session confirmation. Protected-file guards working while disabled
   do not establish active policy enforcement. Respect a declined review.
4. Call `status` and `doctor`. Resolve the concrete reported setup gaps through
   their owning tools. Distinguish enforcement mode, hook publication and current
   host loading. Report any required agent reload; installed files or a responding
   MCP connection do not prove live interception.

For a combined Hub and Shield setup, use Hub's `project-setup` skill from the
Hub server to initialize its project, register
the existing directory and publish its agent configuration. Do not stop at a
list of missing configuration when the user requested setup.

A setup request alone does not request additional packs, custom command
allowances, updates, application services or dependency installation. Finish
after the integration checks unless the user requested permission configuration.
If a required MCP connection is absent, use the product's documented connection
workflow and report the required host reload precisely.

## Configure development permissions

When the user asks to allow the project's development commands, inventory their
effects and use the reviewed steps below. Add only what is missing; preserve
existing packs and rules. Requested pack installation uses the user's selected
pack, profile and settings rather than a repository-wide inventory.

## 1. Read the current policy

Call `pack_list` and `rule_list`. Skip packs and rules that already exist in
later steps.

## 2. Build the inventory

Build it for yourself; do not report it. Shield applies one policy to the whole
repository, so cover every package of a monorepo.

- Find the packages: workspace manifests (Cargo workspace members,
  pnpm-workspace.yaml, package.json workspaces, go.work, composer.json, Gradle
  settings) and any folder with its own manifest.
- For each package, note its stack and package manager from manifests and
  lockfiles.
- Collect its development commands where its tools define them: Makefiles,
  justfiles and other task runners; package.json scripts; composer.json
  scripts; Cargo commands and aliases in .cargo/config.toml; pyproject.toml
  tool sections; Xcode, Gradle and compose files for local services. README,
  CONTRIBUTING, agent instructions and CI workflows show which commands the
  project actually uses.
- Record each command as agents run it from the root, such as
  `make -C packages/api test`, `pnpm --dir web run lint` or
  `cargo test -p acme-core`, and the executables it runs underneath.

## 3. Check what each command runs

Read each target or script and everything it calls: npm pre and post scripts,
included Makefiles, scripts under scripts/ and tools they launch. Every project
is set up differently; judge the code, not the name.

- Keep local development work: unit and integration tests, formatting, lint,
  type checks, builds, doctor and environment checks, dev servers, local
  databases and services, simulators and emulators.
- Drop release, publish, deploy, push, tagging, installation onto the system,
  credential and secret handling, remote environments, destructive cleanup and
  targets that mix kept work with any of these.
- Drop anything you cannot confirm. It keeps asking, which is safe.

## 4. Install packs

1. Look up each kept executable, including those run underneath wrappers, with
   `pack_search`. Confirm with `pack_info` that a match fits the project's
   tools and package manager.
2. Tell the user the packs to install, one line each with its reference and
   what it covers, for example: acme/rust — cargo, rustfmt and Clippy for
   the Rust crates.
3. Call `pack_add` with all of them in `packs` and `review: true`, using the default profile.
   Request optional `variables` only when the project needs one; the form
   collects required settings.
4. Follow any returned `review_action` and wait for the human confirmation
   outcome. The host prompt supplies the decision.

Skip installation when no pack is missing.

## 5. Configure local tool paths

1. Check installed packs for declared executable discovery. For a pack already
   installed, call `pack_configure` with its reference and no edits to discover
   missing settings. Pack add already discovers paths for new selections. Use
   the returned review action if there is a proposal.
2. For custom installations outside the pack's discovery locations, use the
   exact executable paths from the development commands or the user's tool
   configuration. Check the pack's variable declarations. Where a pack accepts
   a list, collect distinct paths for each tool and retain its existing paths.
   Prepare one `pack_configure` change per affected selection with `set` and
   `layer: local`. Follow the ordinary policy review before saving the paths to
   `.shield/config.local.yml`, which is ignored by default.
3. Re-evaluate the commands using their absolute paths. When the corresponding
   command with a standard executable spelling also asks or denies, the
   existing operation rules still apply. Keep those decisions when configuring
   an executable path.

## 6. Add project rules

1. Check each kept command with `policy_evaluate`, as agents would run it.
   Drop those already allowed.
2. Write one `argv` custom rule per remaining command:
   - Write the command the way agents run it. `make -C packages/api test` is
     executable make with the exact arguments -C, packages/api and test, and a
     `cwd` exactly the project root.
   - Use `exact` arguments, or `options` listing only the flags and values you
     reviewed. Never use `prefix` for a script runner: a prefix allows any
     extra arguments.
   - Give each rule a stable descriptive `id`, such as api-make-test, and a
     short `reason` the user reads in the form.
   - Follow the `policy_propose` schema examples for the exact shape.
3. A wrapper such as a make target or package script needs its own rule even
   when a pack allows the tools it runs. Allowing it allows everything it runs,
   now and after later edits, so add only wrappers you read in step 3.
4. Tell the user the commands to allow, one line per base command rather than
   each variant, for example: make -C <package> test, lint and fmt-check for
   packages/api and packages/worker.
5. Call `policy_propose` with one `rule_add` item per rule in `changes` and an
   `agent_reason` naming the files that define them. A proposal holds up to 16
   changes; apply each before preparing the next.
6. Check the proposal: `policy_evaluate` a few intended commands and one near
   miss, such as an extra flag, passing its `after_candidate_id` as
   `candidate_id`. If the scope is wrong, close it with `policy_withdraw` and
   propose a corrected one.
7. Call `policy_review` with the `proposal_id`. For hosts with native tool
   approval, use the unchanged `apply_arguments` with `policy_apply`.

## 7. Finish

- On decline or cancellation, stop that step; do not retry or broaden it. If
  the user declines packs, do not replace them with custom rules for the same
  tools.
- When a proposal is stale, prepare it again from the current policy.
- Do not finish with an undecided proposal; close an abandoned one with
  `policy_withdraw`.
- Report in a few lines what was installed and allowed, and which commands you
  left asking and why.
