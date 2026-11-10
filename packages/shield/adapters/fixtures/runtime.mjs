import { mkdtemp, writeFile, readFile, realpath, rm, mkdir } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { execFile } from 'node:child_process';
import { promisify } from 'node:util';
import { after } from 'node:test';

// Callback transports inherit their application's environment. Establish one
// test-process identity before registration, rather than changing HOME between
// initialization, operator prompts and callbacks. Its default cache is external
// to every enrolled project, just as it is in a real user's home.
export const home = await realpath(await mkdtemp(join(tmpdir(), 'shield-adapter-home-acme-')));
export const fixtureEnvironment = Object.freeze({
  HOME: home, USERPROFILE: home,
  XDG_CACHE_HOME: join(home, '.cache'), XDG_CONFIG_HOME: join(home, '.config'),
  XDG_DATA_HOME: join(home, '.local/share'), XDG_STATE_HOME: join(home, '.local/state'),
  APPDATA: join(home, 'AppData/Roaming'), LOCALAPPDATA: join(home, 'AppData/Local'),
});
Object.assign(process.env, fixtureEnvironment);
after(() => rm(home, { recursive: true, force: true }));

export const binary = process.env.SHIELD_TEST_BINARY || fileURLToPath(new URL('../../target/release/shield', import.meta.url));
const exec = promisify(execFile);
const terminal = fileURLToPath(new URL('../../tests/fixtures/operator_prompts.py', import.meta.url));

// Test-owned controlling terminals exercise the same confirmation boundary as operators.
export async function operator(root, args, answers = 'yes\n', env = process.env) {
  if (args[0] === 'init' && !args.includes('--no-baseline')) args = [...args, '--no-baseline'];
  if (args[0] === 'init' && !args.includes('--agent')) args = [...args, '--no-setup'];
  const steps = Array.isArray(answers) ? answers : answers.replace(/\n$/, '').split('\n');
  // Keep the fixture's saved/default tool selections before final confirmation.
  if (args[0] === 'mcp' && args[1] === 'setup' && args.includes('--agent') && !args.includes('--remove')) steps.unshift('');
  const output = JSON.parse((await exec('python3', [terminal, binary,
    JSON.stringify([...args, '--project', root]), `@${JSON.stringify(steps)}`],
  { timeout: 30000, env: { ...env, TERM: 'xterm-256color', NO_COLOR: '1' } })).stdout);
  if (output.code !== 0) throw new Error(output.stderr || output.terminal);
  return { output: output.terminal };
}

export async function initialize(root, config, env = process.env, answers = 'yes\n') {
  const candidate = JSON.parse(await readFile(config, 'utf8'));
  await mkdir(join(root, '.shield'), { recursive: true });
  if (candidate.rules) {
    await writeFile(join(root, '.shield/rules.yml'), await readFile(join(root, candidate.rules)));
    candidate.rules = 'rules.yml';
  }
  for (const pack of candidate.packs) {
    await writeFile(join(root, '.shield', pack.path), await readFile(join(root, pack.path)));
  }
  await writeFile(join(root, '.shield/config.yml'), JSON.stringify(candidate));
  await operator(root, ['init'], answers, env);
  return JSON.parse((await exec(binary, ['status', '--json', '--project', root], { env })).stdout).active;
}

export async function reviewProposal(root, id, action = 'accept', env = process.env) {
  const state = JSON.parse(await readFile(join(root, '.shield/review/state.json'), 'utf8'));
  const pending = state.proposals.filter(proposal => proposal.status === 'pending');
  if (!pending.some(proposal => proposal.proposal_id === id)) throw new Error('Pending fixture proposal not found');
  // The UI reviews each pending proposal in order; leave unrelated proposals pending.
  return operator(root, ['policy', 'review'], ['review', ...pending.map(proposal => proposal.proposal_id === id ? action : 'leave')], env);
}

export const historySettings = (root, enabled = true, purge = false, env = process.env) =>
  operator(root, ['policy', 'review'], `history\n${enabled ? 'yes' : 'no'}\n5000\n${purge ? 'yes' : 'no'}\nyes\n`, env);

// Read the canonical document through the CLI, independent of its YAML spelling.
export async function readRules(root, env = process.env) {
  return JSON.parse((await exec(binary, ['policy', 'show', '--json', '--project', root], { env })).stdout).project_rules;
}

export async function reactivate(root, env = process.env) {
  const path = join(root, '.shield/rules.yml');
  const rules = await readRules(root, env);
  rules.rules.push({ id: `fixture-${rules.rules.length}`, decision:'deny',
    reason:'Acme unrelated policy change', conditions:{ domain:'argv', executable:'acme-unrelated', arguments:{kind:'any'} } });
  await writeFile(path, JSON.stringify(rules));
  await operator(root, ['policy', 'review'], 'manual\nyes\n', env);
  return JSON.parse((await exec(binary, ['status', '--json', '--project', root], { env })).stdout).active;
}

export async function enrolledRuntime(t, { env: selectedEnvironment = process.env } = {}) {
  const env = { ...selectedEnvironment };
  const root = await realpath(await mkdtemp(join(tmpdir(), 'shield-host-acme-')));
  t.after(() => rm(root, { recursive: true, force: true }));
  const allowed = join(root, 'allowed.txt');
  const denied = join(root, 'denied.txt');
  const asked = join(root, 'asked.txt');
  for (const [path, value] of [[allowed, 'ACME_ALLOWED_TEXT'], [denied, 'ACME_DENIED_TEXT'], [asked, 'ACME_ASKED_TEXT']]) {
    await writeFile(path, value);
  }
  const rule = (id, decision, path) => ({ id, decision, reason: `Acme ${id}`,
    conditions: { domain: 'file', action: 'read', sources: { all: { exact: { root: 'absolute', path } } } } });
  const rules = { schema_version: 1, kind: 'custom', capabilities: ['structured-rules-v1', 'argument-rules-v1', 'location-v1'],
    rules: [rule('read', 'allow', allowed), rule('deny', 'deny', denied),
      { id: 'shell', decision: 'allow', reason: 'Unreachable literal shell allowance',
        conditions: { domain: 'argv', executable: 'Bash', arguments: { kind: 'any' } } },
      { id: 'write', decision: 'allow', reason: 'Unreachable literal write allowance',
        conditions: { domain: 'argv', executable: 'Write', arguments: { kind: 'any' } } }] };
  const config = join(root, 'config.json');
  await writeFile(join(root, 'rules.json'), JSON.stringify(rules));
  await writeFile(config, JSON.stringify({ schema_version: 1, packs: [], rules: 'rules.json' }));
  const cli = async args => JSON.parse((await exec(binary, [...args, '--json', '--project', root], { env })).stdout);
  // The history reader's event view, with reader-derived completion links.
  const events = async () =>
    JSON.parse((await exec(binary, ['__internal', 'history-events', '--project', root], { env })).stdout).events;
  const active = await initialize(root, config, env);
  const statePath = join(root, '.shield/review/state.json');
  const state = await readFile(statePath);
  return { root, allowed, denied, asked, cli, events, active, statePath, state,
    operator: (args, answers) => operator(root, args, answers, env),
    historySettings: (enabled, purge) => historySettings(root, enabled, purge, env),
    readRules: () => readRules(root, env),
    reactivate: () => reactivate(root, env),
    initialize: (answers) => initialize(root, config, env, answers) };
}

export function hostInput(root, tool, input, id = 'tool_acme_1') {
  return { session_id: 'acme-session', transcript_path: join(root, 'transcript.jsonl'), cwd: root,
    permission_mode: 'default', hook_event_name: 'PreToolUse', tool_name: tool, tool_input: input, tool_use_id: id };
}

export function authorizationState(bytes) {
  const state = JSON.parse(bytes);
  // Source-stat hints are disposable cache metadata, not authorization state.
  if (state.active?.compiled) delete state.active.compiled.input_hints;
  return { project_id: state.project_id, project_root: state.project_root, candidates: state.candidates,
    policy_revision: state.policy_revision, active: state.active, proposals: state.proposals,
    authorization_events: state.authorization_events };
}
