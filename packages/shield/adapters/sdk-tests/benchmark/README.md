# Agent benchmarks

A benchmark drives a real Claude Code or Codex agent, with its real model, through
a fixed scenario in a fresh project protected by Shield. A separate observer checks
what actually happened and scores the run. The agent is never told it is being
benchmarked, given follow-up prompts or asked to verify its own work.

A scenario benchmarks either Shield itself or one permission pack. Benchmarks spend
model usage and are never part of `npm test`. Run them on request.

## Layout

| Path | Owns |
| --- | --- |
| `run.mjs` | Runs a scenario with one host and decision set, then scores it |
| `score.mjs` | Scores a saved run with its scenario's current checks; `--record` commits its summary |
| `projection.mjs` | Captures what each host sends its model for Shield's tools, with a scripted local model |
| `lib/scenario.mjs` | Loads and validates a scenario folder and the packs it installs |
| `lib/project.mjs` | The run's project: layout, `shield init`, packs under test, tool stubs, owned processes |
| `lib/session.mjs` | The run loop: driver, turns, observation after each turn, cleanup |
| `lib/observe.mjs` | Shield, Git, configuration, process and tool-stub state after a turn |
| `lib/decisions.mjs` | Answers host requests from the scenario's decision set |
| `lib/transcript.mjs` | What the host delivered to its model and what the model called |
| `lib/checks.mjs` | Checks shared by every scenario, the check kit, verdicts and policy friction |
| `lib/results.mjs` | Neutral summaries and generated result pages |
| `lib/tool-runner.mjs` | Runs a scenario's tool stub in place of the real program and logs the call |
| `drivers/` | Claude Code through the Agent SDK; Codex through its app-server |
| `scenarios/` | Shield's own scenarios |

## Scenarios

A scenario is a folder with two files:

- `scenario.json`: the data. `name`, `description`, `subject` (`shield` or a pack
  ID), `results` (the results folder, relative to the scenario), `turns` (1–10
  prompts with IDs), `decisions` (named sets of answers to policy confirmations,
  by offer kind, with `other` as the fallback) and `limits` (`turn_minutes`,
  `run_minutes`, `claude_max_budget_usd`, `codex_max_total_tokens`). Optional:
  `project` (folder name), `seed` (project files), `support` (files in the run
  folder outside the project), `environment` (variables the agent's commands see),
  `tools` (stubs, below), `packs` and `offers`.
- `scenario.mjs`: the code. It exports `checks(run, context, kit)` returning
  check results, and optionally `hooks.afterTurn` (extra observations recorded on
  each turn) and `hooks.afterRun` (the harness's own verification after the agent
  stops).

`{run}` and `{project}` in `support` and `environment` values name the run's
folders. `offers` maps offer kinds to regular expressions over the confirmation
text; the first match names the offer, and unmatched offers are `other`.

`packs` lists packs to install after `shield init`, each with `source` (a pack
folder, or a pack's parent folder whose newest version is used), `profile` and
`variables`. They are served from a run-local registry folder, so unpublished
versions work. A pack subject must be one of these packs.

`tools` maps a command name to `{module, environment_prefix}`. The harness puts a
shim first on the agent's `PATH` that runs the module's default export,
`(args, {env, cwd}) => {stdout, stderr, code, effective}`, instead of the real
program, and logs each call with the environment variables carrying the prefix.
`effective` is the stub's own account of what the call did, for the checks.

`checks` receives the saved run, a `context` with the final observation, Shield's
decisions and asks by turn, the model's view, confirmations and tool calls by turn,
and a `kit` with `check(id, status, attribution, summary, evidence)`, `invokes`,
`turnOf`, `strings` and `PAST`. Statuses are `pass`, `fail` and `unverified`;
attributions are `shield`, `agent`, `host` and `harness`. Shared checks add
decision-table conformance, approved-rule persistence, unchanged agent
configuration, unanswered host requests and process cleanup.

## What a run does

1. Creates a fresh project under `~/ShieldSessions/runs/<time>-<label>/`, with a
   private compiled-policy cache, diagnostic logs, support files and tool stubs.
2. Runs the real `shield init` through a test-owned terminal, with Baseline's
   Recommended profile and the selected agent's integration, then installs the
   scenario's packs. The run stops if the registered binary digest differs from
   the executable under test.
3. Starts the agent with an isolated configuration folder and sends each prompt
   unchanged, waiting for each turn to finish.
4. Answers every host approval from the decision set. Ordinary task commands are
   approved once. Policy confirmations follow the set's answer for their offer
   kind. Questions to the user, and other requests without a scripted answer, are
   refused and recorded.
5. After each turn, records Shield history, proposals, active custom rules,
   `suggestion_list` recurrence for each session, Git contents, run-owned
   processes and ports, configuration file digests and tool-stub calls, then runs
   the scenario's `afterTurn` hook.
6. Stops the agent and every process whose working directory is inside the
   project, then runs the scenario's `afterRun` hook.
7. Scores the run and writes `report.md` and `report.json`, including policy
   friction: how many requests Shield allowed, asked about and denied per turn.
8. With `--record`, writes the run's neutral summary to the scenario's results
   folder under the subject's version (Shield's or the pack's) and regenerates its
   pages.

## Before the first run

Log in once per host with the account the runs should use. The homes stay outside
your normal configuration and are reused by later runs:

```sh
CLAUDE_CONFIG_DIR=~/ShieldSessions/agents/claude claude   # then /login and /exit
CODEX_HOME=~/ShieldSessions/agents/codex codex login
```

Install the Shield build under test, for example with
`make -C packages/shield install-local`. Runs use `shield` from `PATH` unless
`--shield` names another executable.

## Run and record

From `packages/shield/adapters/sdk-tests/`:

```sh
npm run benchmark -- --host claude --decisions claude-confirmation --record --role confirmation
npm run benchmark -- --host codex --decisions codex-confirmation --record --role confirmation
npm run benchmark -- --host claude --decisions default --scenario /path/to/scenarios/aws --record
```

`--scenario` takes one of Shield's scenario names (default `development-workflow`)
or a scenario folder path. `--note` adds a short explanation to the recorded
result. Leave out `--record` for exploratory runs, and record a saved run later
with `npm run benchmark:record -- ~/ShieldSessions/runs/<run> --role <role>`.
`--setup-only` prepares and observes the fixture, packs included, without
starting an agent. `--runs`, `--agent-home` and `--label` override the defaults.
Inherited API keys and base URLs are removed, so runs use the logged-in account.
The scenario's limits stop a run that exceeds its turn time, total time, Claude
cost estimate or Codex token count.

`npm run benchmark:projection` captures the tool schemas and results each host
sends its model, using a local scripted model, without spending usage.

## Results

Real models vary between runs, so checks assert outcomes that must hold every
time rather than exact wording or tool order. Unverified means the evidence a
check needs did not occur; it is not a pass.

Rescore a saved run after changing a check, without running the agent again:

```sh
npm run benchmark:score -- ~/ShieldSessions/runs/<run>
npm run benchmark:test
```

A recorded result keeps the subject (Shield, or the pack's ID, version and
digest), the Shield version and digest, host, model, platform, packs, conditions
and decisions. It also keeps each check's outcome, policy friction, the offers
and their answers, and per-turn time and usage. The runner saves the executed
scenario folder digest in `run.json` as `scenario_sha256`. Summary schema 3 keeps
that digest and separately records `scoring_scenario_sha256` for the current
scenario used to rescore the run; `oracle_sha256` identifies the current scoring
code and scenario. Editing prompts or fixtures before rescoring does not change
the saved execution identity. For older runs without a saved execution digest,
`scenario_sha256` is `null`; the current files cannot establish it. Existing
recorded summaries are preserved. Paths and session IDs are replaced with
placeholders.

A run folder holds the raw host event stream, transcript copy, tool-stub log and
observations. These can contain model output and local paths. Keep them private
and never commit them.

## Limits

Claude Code runs through the Agent SDK rather than its interactive terminal. Codex
runs through its app-server with the project marked trusted and Shield's generated
hooks accepted without the interactive `/hooks` review. Both are recorded in each
run's conditions. A tool stub replaces a program found through `PATH`; a command
naming the real program's absolute path bypasses it, so pack scenarios also point
the real program's configuration at fake credentials. Codex normally runs commands
in a login shell, where macOS `path_helper` puts system folders ahead of the stub
folder, so scenarios with tool stubs disable Codex login shells and let its
sandbox write the stub call log. The shared `harness.stub.<tool>` check fails a
run in which Shield let a stubbed tool's commands run but the stub logged none.
Runs are verified on macOS only.
