import { mkdtemp, writeFile, readFile, realpath, rm, mkdir } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { execFile } from 'node:child_process';
import { promisify } from 'node:util';

export const binary = fileURLToPath(new URL('../../target/release/shield', import.meta.url));
const exec = promisify(execFile);
const terminal = fileURLToPath(new URL('../../tests/fixtures/operator_prompts.py', import.meta.url));

// Test-owned controlling terminals exercise the same confirmation boundary as operators.
export async function operator(root, args, answers = 'yes\n') {
  if (args[0] === 'init' && !args.includes('--no-baseline')) args = [...args, '--no-baseline'];
  if (args[0] === 'init' && !args.includes('--agent')) args = [...args, '--no-setup'];
  if (args[0] === 'init' && !args.includes('--cache-dir')) args = [...args, '--cache-dir', join(root, '.shield/cache')];
  const steps = Array.isArray(answers) ? answers : answers.replace(/\n$/, '').split('\n');
  // Keep the fixture's saved/default tool selections before final confirmation.
  if (args[0] === 'mcp' && args[1] === 'setup' && args.includes('--agent') && !args.includes('--remove')) steps.unshift('');
  const output = JSON.parse((await exec('python3', [terminal, binary,
    JSON.stringify([...args, '--project', root]), `@${JSON.stringify(steps)}`],
  { timeout: 30000, env: { ...process.env, HOME: root, TERM: 'xterm-256color', NO_COLOR: '1' } })).stdout);
  if (output.code !== 0) throw new Error(output.stderr || output.terminal);
  return { output: output.terminal };
}

export async function initialize(root, config) {
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
  await operator(root, ['init']);
  return JSON.parse((await exec(binary, ['status', '--json', '--project', root])).stdout).active;
}

export async function reviewProposal(root, id, action = 'accept') {
  const state = JSON.parse(await readFile(join(root, '.shield/review/state.json'), 'utf8'));
  const pending = state.proposals.filter(proposal => proposal.status === 'pending');
  if (!pending.some(proposal => proposal.proposal_id === id)) throw new Error('Pending fixture proposal not found');
  // The UI reviews each pending proposal in order; leave unrelated proposals pending.
  return operator(root, ['policy', 'review'], ['review', ...pending.map(proposal => proposal.proposal_id === id ? action : 'leave')]);
}

export const historySettings = (root, enabled = true, purge = false) =>
  operator(root, ['policy', 'review'], `history\n${enabled ? 'yes' : 'no'}\n5000\n${purge ? 'yes' : 'no'}\nyes\n`);

// Read the canonical document through the CLI, independent of its YAML spelling.
export async function readRules(root) {
  return JSON.parse((await exec(binary, ['policy', 'show', '--json', '--project', root])).stdout).project_rules;
}

export async function reactivate(root) {
  const path = join(root, '.shield/rules.yml');
  const rules = await readRules(root);
  rules.rules.push({ id: `fixture-${rules.rules.length}`, decision:'deny',
    reason:'Acme unrelated policy change', conditions:{ domain:'argv', executable:'acme-unrelated', arguments:{kind:'any'} } });
  await writeFile(path, JSON.stringify(rules));
  return operator(root, ['policy', 'review'], 'manual\nyes\n');
}

export async function enrolledRuntime(t) {
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
  const cli = async args => JSON.parse((await exec(binary, [...args, '--json', '--project', root])).stdout);
  // The history reader's event view, with reader-derived completion links.
  const events = async () =>
    JSON.parse((await exec(binary, ['__internal', 'history-events', '--project', root])).stdout).events;
  const active = await initialize(root, config);
  const statePath = join(root, '.shield/review/state.json');
  const state = await readFile(statePath);
  return { root, allowed, denied, asked, cli, events, active, statePath, state,
    operator: (args, answers) => operator(root, args, answers),
    historySettings: (enabled, purge) => historySettings(root, enabled, purge),
    readRules: () => readRules(root),
    reactivate: () => reactivate(root),
    initialize: () => initialize(root, config) };
}

export function hostInput(root, tool, input, id = 'tool_acme_1') {
  return { session_id: 'acme-session', transcript_path: join(root, 'transcript.jsonl'), cwd: root,
    permission_mode: 'default', hook_event_name: 'PreToolUse', tool_name: tool, tool_input: input, tool_use_id: id };
}

export function authorizationState(bytes) {
  const state = JSON.parse(bytes);
  return { project_id: state.project_id, project_root: state.project_root, candidates: state.candidates,
    policy_revision: state.policy_revision, active: state.active, proposals: state.proposals,
    authorization_events: state.authorization_events };
}
