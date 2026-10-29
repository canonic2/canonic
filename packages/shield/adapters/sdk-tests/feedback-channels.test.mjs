// Verify Claude receives proposal guidance separately from its visible hook error.
import test from 'node:test';
import assert from 'node:assert/strict';
import { writeFile, mkdtemp, realpath, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { execFile } from 'node:child_process';
import { promisify } from 'node:util';
import { query } from '@anthropic-ai/claude-agent-sdk';
import { enrolledRuntime } from '../fixtures/runtime.mjs';
import { scriptedProvider } from './claude-provider.mjs';

test('Claude receives denied-command guidance separately from its visible refusal', { timeout: 60000 }, async t => {
  const f = await enrolledRuntime(t);
  const outside = await realpath(await mkdtemp(join(tmpdir(), 'shield-feedback-acme-')));
  t.after(() => rm(outside, { recursive: true, force: true }));
  const path = join(outside, 'job.output');
  await writeFile(path, 'ACME_PRIVATE_OUTPUT');
  const rulesPath = join(f.root, '.shield/rules.yml');
  const rules = await f.readRules();
  rules.capabilities.push('requirements-v1', 'access-paths-v1');
  rules.rules.push({ id: 'scoped-reads', decision: 'allow', conditions: { domain: 'file', action: 'read' },
    requirements: { targets: { action: 'read', all: { within: { root: 'read' } } } }, on_unmet: { decision: 'deny' } });
  rules.rules.push({ id: 'denied-cat', decision: 'deny', conditions: { domain: 'argv', executable: 'cat', arguments: { kind: 'any' } } });
  await writeFile(rulesPath, JSON.stringify(rules));
  await f.operator(['policy', 'review'], 'manual\nyes\n');
  await f.operator(['mcp', 'setup', '--agent', 'claude']);
  const provider = await scriptedProvider(t, [{ name: 'Bash', input: { command: `cat '${path}'` } }]);
  const executable = (await promisify(execFile)('which', ['claude'])).stdout.trim();
  const conversation = query({ prompt: 'Read the Acme task output.', options: {
    pathToClaudeCodeExecutable: executable, permissionMode: 'default', cwd: f.root,
    settingSources: ['project', 'local'], tools: ['Bash'], persistSession: false, maxTurns: 3,
    canUseTool: async () => assert.fail('A denied read must not open a permission prompt.'),
    env: { ...process.env, ANTHROPIC_API_KEY: 'acme-local-test-key', ANTHROPIC_BASE_URL: provider.url,
      CLAUDE_CONFIG_DIR: join(f.root, 'claude-config'), CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC: '1' },
  } });
  t.after(() => conversation.close());
  const messages = [];
  for await (const message of conversation) messages.push(message);
  const visible = messages.filter(message => message.type === 'user').flatMap(message => message.message.content ?? [])
    .filter(block => block.type === 'tool_result');
  const error = JSON.stringify(visible);
  assert.match(error, /Shield: blocked/);
  assert.match(error, /active policy does not allow/);
  assert.doesNotMatch(error, /policy_propose|source_entry_id|apply_arguments|access_path_add|ACME_PRIVATE_OUTPUT/);
  const modelInput = JSON.stringify(provider.requests);
  assert.match(modelInput, /policy_propose/);
  assert.match(modelInput, /policy_apply/);
});
