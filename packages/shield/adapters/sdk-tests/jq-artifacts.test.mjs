// Execute the reported jq forms through the real native Claude hook boundary.
import test from 'node:test';
import assert from 'node:assert/strict';
import { writeFile, mkdir } from 'node:fs/promises';
import { join } from 'node:path';
import { execFile } from 'node:child_process';
import { promisify } from 'node:util';
import { query } from '@anthropic-ai/claude-agent-sdk';
import { enrolledRuntime } from '../fixtures/runtime.mjs';
import { scriptedProvider } from './claude-provider.mjs';

const exec = promisify(execFile);
const quote = value => `'${value.replaceAll("'", "'\\''")}'`;
const summary = '.data | "desc: \\(.description)\\nprofiles: \\(.profiles)\\nrules: \\(.rules|length)\\nrule keys: \\(.rules[0]|keys)"';
const listing = '.data.rules[] | select(.id|test("npm")) | [.id, (.profile_decisions|tostring), .reason[0:90]] | @tsv';

test('Claude reads its own tool results with jq interpolation and selection without approvals', { timeout: 60000 }, async t => {
  const f = await enrolledRuntime(t);
  const rulesPath = join(f.root, '.shield/rules.yml');
  const rules = await f.readRules();
  rules.capabilities.push('requirements-v1', 'access-paths-v1');
  rules.rules.push({ id: 'project-and-session-reads', decision: 'allow', conditions: { domain: 'file', action: 'read' },
    requirements: { targets: { action: 'read', all: { within: { root: 'read' } } } }, on_unmet: { decision: 'deny' } },
  { id: 'head', decision: 'allow', conditions: { domain: 'argv', executable: 'head', arguments: { kind: 'any' } } });
  await writeFile(rulesPath, JSON.stringify(rules));
  await f.operator(['policy', 'review'], 'manual\nyes\n');
  await f.operator(['mcp', 'setup', '--agent', 'claude']);
  let resolveResult;
  const resultReady = new Promise(resolve => { resolveResult = resolve; });
  const provider = await scriptedProvider(t, [
    { name: 'Bash', input: async () => ({ command: `jq -r ${quote(summary)} ${quote(await resultReady)}` }) },
    { name: 'Bash', input: async () => ({ command: `jq -r ${quote(listing)} ${quote(await resultReady)} | head -60` }) },
    { name: 'Read', input: async () => ({ file_path: await resultReady }) },
  ]);
  const prompts = [];
  const config = join(f.root, 'claude-config');
  const executable = (await exec('which', ['claude'])).stdout.trim();
  const conversation = query({ prompt: 'Inspect the Acme tool result with the supplied tools.', options: {
    pathToClaudeCodeExecutable: executable, permissionMode: 'default',
    cwd: f.root, settingSources: ['project', 'local'], tools: ['Bash', 'Read'],
    persistSession: false, maxTurns: 6,
    canUseTool: async name => { prompts.push(name); return { behavior: 'deny', message: 'Unexpected approval.' }; },
    env: { ...process.env, ANTHROPIC_API_KEY: 'acme-local-test-key', ANTHROPIC_BASE_URL: provider.url,
      CLAUDE_CONFIG_DIR: config, CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC: '1' },
  } });
  t.after(() => conversation.close());
  const messages = [];
  for await (const message of conversation) {
    messages.push(message);
    if (message.type === 'system' && message.subtype === 'init') {
      const key = f.root.replaceAll(/[^A-Za-z0-9]/g, '-');
      const directory = join(config, 'projects', key, message.session_id, 'tool-results');
      await mkdir(directory, { recursive: true });
      const path = join(directory, 'mcp-shield-info-acme.txt');
      await writeFile(path, JSON.stringify({ data: { description: 'Acme inspection', profiles: ['recommended'],
        rules: [{ id: 'npm-install', profile_decisions: { recommended: 'allow' }, reason: 'Acme local installation' }] } }));
      resolveResult(path);
    }
  }
  assert.ok(messages.some(message => message.type === 'result' && message.subtype === 'success'), JSON.stringify(messages));
  assert.deepEqual(prompts, [], JSON.stringify(provider.results));
  const output = JSON.stringify(provider.results);
  assert.match(output, /desc: Acme inspection/);
  assert.match(output, /npm-install/);
  assert.doesNotMatch(output, /hook error|approval required|denied by active policy/i);
  const decisions = (await f.events()).filter(event => event.kind === 'decision');
  assert.equal(decisions.length, 3);
  assert.ok(decisions.every(event => event.details.decision === 'allow'), JSON.stringify(decisions));
});
