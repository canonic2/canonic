---
name: project-setup
description: Configure an initialized Shield project for development by inventorying its packages and development commands, installing matching packs and allowing the verified commands as project rules.
---

# Configure Shield for this project

Use when the user asks to set up, configure or refresh Shield for this project.
The result: agents can run the project's everyday development commands without
asking, and everything else still asks. Every run only adds what is missing; it
never removes, narrows or updates existing packs and rules.

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
3. Call `pack_add` with all of them in `packs`, using the default profile.
   Request optional `variables` only when the project needs one; the form
   collects required settings.
4. Copy `apply_arguments` unchanged to `policy_apply`. Its prompt is the
   approval; do not also ask in chat.

Skip this step when no pack is missing.

## 5. Add project rules

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
7. Copy `apply_arguments` unchanged to `policy_apply`. Its form is the
   approval; do not also ask in chat.

## 6. Finish

- On decline or cancellation, stop that step; do not retry or broaden it. If
  the user declines packs, do not replace them with custom rules for the same
  tools.
- When a proposal is stale, prepare it again from the current policy.
- Do not finish with an undecided proposal; close an abandoned one with
  `policy_withdraw`.
- Report in a few lines what was installed and allowed, and which commands you
  left asking and why.
