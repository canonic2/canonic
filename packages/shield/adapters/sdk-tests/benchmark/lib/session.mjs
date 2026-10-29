// One benchmark run: fresh project, Shield and packs under test, a real agent driven
// through the scenario's turns, independent observation after each, then cleanup.
import { appendFile, readFile, writeFile } from 'node:fs/promises';
import { arch, release } from 'node:os';
import { join } from 'node:path';
import { startClaude } from '../drivers/claude.mjs';
import { startCodex } from '../drivers/codex.mjs';
import { decisions as answers } from './decisions.mjs';
import { observe, packRows, toolCalls } from './observe.mjs';
import { agentEnvironment, configurationDigests, createRun, digestFile, exec, initialize, installPacks, shieldJson, stopOwned } from './project.mjs';

const CONDITIONS = {
  claude: ['Claude Code driven through the Agent SDK (claude_code system prompt, project and local settings), not its interactive terminal.',
    'claude.ai account connectors disabled through flag settings.'],
  codex: ['Agent SDK not used; Codex app-server with approval policy on-request and workspace-write sandbox.',
    'Project marked trusted and generated hooks accepted without the interactive /hooks review.'],
};

/** Runs the scenario and returns the saved run record and its folder. */
export async function runScenario({ scenario, host, decisionSet, shield, executable, home, runs, label, setupOnly }) {
  const decisions = answers(scenario, decisionSet);
  const stubbed = Object.keys(scenario.tools).length > 0;
  const run = await createRun(runs, label, scenario);
  const events = join(run.root, 'events.jsonl');
  const record = value => appendFile(events, `${JSON.stringify({ at: new Date().toISOString(), ...value })}\n`).catch(() => {});
  const deadline = Date.now() + scenario.limits.run_minutes * 60000;
  const output = {
    label, host, scenario: scenario.name, scenario_directory: scenario.directory, decision_set: decisionSet,
    scenario_sha256: scenario.sha256,
    root: run.root, project: run.project,
    identities: {
      shield, shield_sha256: await digestFile(shield), shield_version: (await exec(shield, ['--version'])).stdout.trim(),
      host_executable: executable, host_cli: (await exec(executable, ['--version'])).stdout.trim(),
      node: process.version, agent_home: home, platform: { os: process.platform, release: release(), arch: arch() },
      subject: scenario.subject.kind === 'shield' ? { kind: 'shield' }
        : { kind: 'pack', id: scenario.subject.id, version: scenario.subject.version, sha256: scenario.subject.sha256 },
    },
    conditions: [...CONDITIONS[host], ...(stubbed
      ? [`Stubbed tools on PATH: ${Object.keys(scenario.tools).join(', ')}; their calls never reach real services.`,
        ...(host === 'codex' ? ['Codex login shells disabled so the stubs stay first on PATH.'] : [])] : [])],
    turns: [], decision_log: decisions.log, decision_table: scenario.decisions[decisionSet],
  };
  const save = () => writeFile(join(run.root, 'run.json'), `${JSON.stringify(output, null, 2)}\n`);
  let driver;
  try {
    output.init_terminal = await initialize(shield, run, host);
    output.pack_terminals = await installPacks(shield, run, scenario.packs);
    output.initial_config = await configurationDigests(run.project);
    const status = await shieldJson(shield, run.project, ['status']);
    output.identities.policy_revision = status.policy_revision;
    output.identities.packs = packRows(status.installed_packs?.packs);
    output.identities.registered_binary_digest = JSON.parse(await readFile(join(run.project, '.shield/review/state.json'), 'utf8')).operator?.integration?.binary_digest;
    if (output.identities.registered_binary_digest !== output.identities.shield_sha256) throw new Error('Registered Shield digest does not match the executable under test');
    if (setupOnly) {
      output.setup_observation = await observe({ shield, run, sessionIds: [] });
      return { output, root: run.root };
    }

    const start = host === 'claude' ? startClaude : startCodex;
    driver = await start({ project: run.project, home, logs: run.logs, executable, decisions, limits: scenario.limits, record,
      environment: agentEnvironment(run, scenario), stubbedTools: stubbed, writableRoots: stubbed ? [run.toolLogs] : [] });
    for (const turn of scenario.turns) {
      if (Date.now() > deadline) throw new Error('run time limit reached');
      const entry = { id: turn.id, prompt: turn.prompt, started_at: new Date().toISOString() };
      output.turns.push(entry);
      entry.result = await driver.turn(turn.prompt);
      entry.finished_at = new Date().toISOString();
      Object.assign(output.identities, { session_id: driver.session.id, model: driver.session.model, host_version: driver.session.version });
      const transcript = await driver.transcript();
      entry.transcript_length = transcript ? (await readFile(transcript, 'utf8')).split('\n').filter(Boolean).length : 0;
      entry.observation = await observe({ shield, run, sessionIds: [driver.session.id].filter(Boolean) });
      Object.assign(entry, await scenario.hooks.afterTurn?.({ turn, observation: entry.observation, driver, project: run.project }));
      await save();
    }
  } catch (error) {
    output.error = { message: error.message, stack: error.stack };
  } finally {
    const transcript = await driver?.transcript().catch(() => null);
    await driver?.close().catch(error => { output.close_error = error.message; });
    output.cleanup = await stopOwned(run.project);
    if (output.turns.length && scenario.hooks.afterRun) {
      output.verification = await scenario.hooks.afterRun({ project: run.project });
      output.verification_cleanup = await stopOwned(run.project);
    }
    output.tool_calls = await toolCalls(run);
    if (transcript) {
      output.transcript = transcript;
      output.transcript_lines = (await readFile(transcript, 'utf8')).split('\n').filter(Boolean).map(line => JSON.parse(line));
    }
    await save();
  }
  return { output, root: run.root };
}
