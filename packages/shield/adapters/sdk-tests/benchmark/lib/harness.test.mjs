// Harness behavior without agents or Shield: scenarios, tool stubs, transcripts and summaries.
import test from 'node:test';
import assert from 'node:assert/strict';
import { execFile } from 'node:child_process';
import { mkdir, mkdtemp, readdir, readFile, rm, writeFile } from 'node:fs/promises';
import { homedir, tmpdir } from 'node:os';
import { join } from 'node:path';
import { promisify } from 'node:util';
import { agentEnvironment, createRun } from './project.mjs';
import { summary } from './results.mjs';
import { loadScenario } from './scenario.mjs';
import { codeModeCalls } from './transcript.mjs';
import { score } from '../score.mjs';

const exec = promisify(execFile);

async function folder(t) {
  const path = await mkdtemp(join(tmpdir(), 'acme-benchmark-'));
  t.after(() => rm(path, { recursive: true, force: true }));
  return path;
}

async function packScenario(root, overrides = {}) {
  for (const version of ['0.1.0', '0.2.0', '0.10.0']) {
    await mkdir(join(root, 'packs/acme/tools', version), { recursive: true });
    await writeFile(join(root, 'packs/acme/tools', version, 'pack.yml'), JSON.stringify({ id: 'acme/tools', version }));
  }
  const directory = join(root, 'scenario');
  await mkdir(join(directory, 'tools'), { recursive: true });
  await writeFile(join(directory, 'scenario.json'), JSON.stringify({ name: 'acme-tools', description: 'Acme tools.', subject: 'acme/tools',
    results: '../results', packs: [{ source: '../packs/acme/tools', variables: { profile: 'acme-ro' } }],
    tools: { acme: { module: 'tools/acme.mjs', environment_prefix: 'ACME_' } }, environment: { ACME_CONFIG: '{run}/acme.conf' },
    support: { 'acme.conf': 'profile={project}\n' }, turns: [{ id: 'one', prompt: 'List Acme things.' }],
    decisions: { default: { other: 'decline' } }, limits: { turn_minutes: 1, run_minutes: 1 }, ...overrides }));
  await writeFile(join(directory, 'scenario.mjs'), 'export const checks = () => [];\n');
  await writeFile(join(directory, 'tools/acme.mjs'),
    'export default args => ({ stdout: JSON.stringify({ args }) + "\\n", code: args[0] === "fail" ? 3 : 0, effective: { first: args[0] } });\n');
  return directory;
}

test('a pack scenario selects the newest pack version and resolves its results folder', async t => {
  const root = await folder(t);
  const scenario = await loadScenario(await packScenario(root));
  assert.equal(scenario.subject.kind, 'pack');
  assert.equal(scenario.subject.version, '0.10.0');
  assert.deepEqual(scenario.packs[0].variables, { profile: 'acme-ro' });
  assert.equal(scenario.results, join(root, 'results'));
  assert.match(scenario.sha256, /^[0-9a-f]{64}$/);
});

test('scenarios refuse more than ten turns and unknown pack subjects', async t => {
  const root = await folder(t);
  const turns = Array.from({ length: 11 }, (_, index) => ({ id: `t${index}`, prompt: 'Acme.' }));
  await assert.rejects(loadScenario(await packScenario(root, { turns })), /turns must number 1-10/);
  await assert.rejects(loadScenario(await packScenario(root, { subject: 'acme/other' })), /not one of its packs/);
});

test('tool stubs answer in place of the real program and log each call', async t => {
  const root = await folder(t);
  const scenario = await loadScenario(await packScenario(root));
  const run = await createRun(root, 'acme', scenario);
  const environment = { ...process.env, ...agentEnvironment(run, scenario), ACME_REGION: 'eu-west-1' };
  assert.equal(environment.ACME_CONFIG, join(run.root, 'acme.conf'));
  assert.equal(await readFile(join(run.root, 'acme.conf'), 'utf8'), `profile=${run.project}\n`);
  const ok = await exec('acme', ['list', '--json'], { env: environment, cwd: run.project });
  assert.deepEqual(JSON.parse(ok.stdout), { args: ['list', '--json'] });
  const failed = await exec('acme', ['fail'], { env: environment }).catch(error => error);
  assert.equal(failed.code, 3);
  const calls = (await readFile(run.toolLog, 'utf8')).trim().split('\n').map(line => JSON.parse(line));
  assert.deepEqual(calls.map(call => [call.tool, call.args, call.code, call.environment.ACME_REGION, call.effective.first]),
    [['acme', ['list', '--json'], 0, 'eu-west-1', 'list'], ['acme', ['fail'], 3, 'eu-west-1', 'fail']]);
  assert.equal(calls[0].environment.PATH, undefined);
});

test('a recorded summary keeps outcomes and drops paths and session identities', async () => {
  const scenario = await loadScenario('development-workflow');
  const root = join(homedir(), 'ShieldSessions/runs/20261027T120000-acme');
  const project = join(root, 'acme-health');
  const run = {
    scenario: 'development-workflow', host: 'claude', decision_set: 'claude-candidate', root, project,
    decision_table: scenario.decisions['claude-candidate'], conditions: [`Fixture at ${project}`],
    identities: { shield_version: 'shield 0.9.0', shield_sha256: 'a'.repeat(64), host_cli: '2.1.311 (Claude Code)',
      host_version: '2.1.311', model: 'acme-model', node: 'v24.12.0', session_id: 'acme-session-1234', packs: [], subject: { kind: 'shield' } },
    turns: [{ id: 'git', started_at: '2026-10-27T12:00:00.000Z', finished_at: '2026-10-27T12:01:30.000Z', result: { status: 'success', cost_usd: 0.4 } }],
    decision_log: [{ kind: 'offer', offer: 'other', action: 'accept', at: '2026-10-27T12:00:30.000Z', text: 'Add the git rule pack — version 0.7.0' }],
  };
  const result = { verdict: 'pass', counts: { pass: 1 }, friction: [{ turn: 'git', allow: 2, ask: 1, deny: 0 }],
    checks: [{ id: 'task.git', status: 'pass', attribution: null, summary: `Commit in ${project} for session acme-session-1234`, evidence: { path: project } }] };
  const entry = await summary(run, result, scenario, { role: 'candidate' });
  const text = JSON.stringify(entry);
  for (const leak of [homedir(), root, project, 'acme-session-1234']) assert.ok(!text.includes(leak), leak);
  assert.equal(entry.shield.version, '0.9.0');
  assert.deepEqual(entry.subject, { kind: 'shield' });
  assert.equal(entry.checks[0].summary, 'Commit in <project> for session <session>');
  assert.equal(entry.checks[0].evidence, undefined);
  assert.deepEqual(entry.offers.map(offer => offer.offer), ['git']);
  assert.equal(entry.totals.seconds, 90);
  assert.equal(entry.role, 'candidate');
});

test('code-mode scripts expose each inner tool call with parsed arguments', () => {
  const calls = codeModeCalls('const r = await tools.mcp__shield__policy_propose({change:{kind:"rule_add",rule:{id:"acme",decision:"allow"}},agent_reason:"Acme (twice)"});\ntext(await tools.exec_command({cmd:"curl -s http://127.0.0.1:3000/health"}));');
  assert.deepEqual(calls.map(call => call.name), ['mcp__shield__policy_propose', 'exec_command']);
  assert.equal(calls[0].input.change.rule.id, 'acme');
  assert.equal(calls[1].input.cmd, 'curl -s http://127.0.0.1:3000/health');
});

test('recording after scenario edits preserves the saved execution identity', async t => {
  const root = await folder(t);
  const directory = await packScenario(root);
  const executed = await loadScenario(directory);
  const run = await createRun(root, 'recording', executed);
  const saved = {
    ...run, scenario: executed.name, scenario_directory: directory, scenario_sha256: executed.sha256,
    host: 'claude', decision_set: 'default', decision_table: executed.decisions.default,
    identities: { shield_version: 'shield 0.1.0', shield_sha256: 'a'.repeat(64), host_cli: 'Acme fixture', subject: executed.subject },
    initial_config: {}, conditions: [], decision_log: [], tool_calls: [], cleanup: { remaining: [] },
    turns: [{ id: 'one', prompt: executed.turns[0].prompt,
      started_at: '2026-10-28T12:00:00.000Z', finished_at: '2026-10-28T12:00:01.000Z',
      result: { status: 'success' }, observation: { history: { entries: [] }, state: { proposals: [] },
        custom_rules: [], config: {}, tool_calls: 0 } }],
  };
  const save = () => writeFile(join(run.root, 'run.json'), JSON.stringify(saved));
  const recorded = async () => {
    const folder = join(executed.results, executed.subject.version);
    const name = (await readdir(folder)).find(name => name.endsWith('.json'));
    return JSON.parse(await readFile(join(folder, name), 'utf8'));
  };
  await save();
  await score(run.root, {});
  const original = await recorded();
  assert.equal(original.scenario_sha256, executed.sha256);
  assert.equal(original.scoring_scenario_sha256, executed.sha256);

  const path = join(directory, 'scenario.json');
  const changed = JSON.parse(await readFile(path, 'utf8'));
  changed.turns[0].prompt = 'Inspect different Acme things.';
  changed.support['acme.conf'] = 'changed fixture\n';
  await writeFile(path, JSON.stringify(changed));
  const scoring = await loadScenario(directory);
  assert.notEqual(scoring.sha256, executed.sha256);
  await score(run.root, {});
  const rescored = await recorded();
  assert.equal(rescored.schema_version, 3);
  assert.equal(rescored.scenario_sha256, executed.sha256);
  assert.equal(rescored.scoring_scenario_sha256, scoring.sha256);
  assert.notEqual(rescored.oracle_sha256, original.oracle_sha256);
  assert.equal(JSON.parse(await readFile(join(run.root, 'run.json'), 'utf8')).scenario_sha256, executed.sha256);

  // An older run cannot acquire a fabricated execution identity from today's files.
  delete saved.scenario_sha256;
  await save();
  await score(run.root, {});
  const legacy = await recorded();
  assert.equal(legacy.scenario_sha256, null);
  assert.equal(legacy.scoring_scenario_sha256, scoring.sha256);
});
