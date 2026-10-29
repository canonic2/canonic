// Actual Claude host tools use temporary roots supplied by its Shield adapter.
import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtemp, readFile, writeFile, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { execFile } from 'node:child_process';
import { promisify } from 'node:util';
import { query } from '@anthropic-ai/claude-agent-sdk';
import { enrolledRuntime } from '../fixtures/runtime.mjs';
import { scriptedProvider } from './claude-provider.mjs';

test('Claude reads and writes arbitrary temporary files through shell and native file tools', { timeout: 60000 }, async t => {
  const f = await enrolledRuntime(t);
  const scratch = await mkdtemp(join(tmpdir(), 'shield-temporary-acme-'));
  t.after(() => rm(scratch, { recursive: true, force: true }));
  const existing = join(scratch, 'background-task.output');
  const native = join(scratch, 'native.txt');
  const shell = join(scratch, 'shell.txt');
  await writeFile(existing, 'ACME_EXISTING_TEMPORARY_OUTPUT');
  const path = join(f.root, '.shield/rules.yml');
  const rules = await f.readRules();
  rules.capabilities.push('requirements-v1', 'access-paths-v1');
  rules.rules.push(
    { id: 'printf', decision: 'allow', conditions: { domain: 'argv', executable: 'printf', arguments: { kind: 'any' } } },
    { id: 'temporary-read', decision: 'allow', conditions: { domain: 'file', action: 'read' }, requirements: { targets: { action: 'read', all: { within: { root: 'read' } } } }, on_unmet: { decision: 'deny' } },
    { id: 'temporary-write', decision: 'allow', conditions: { domain: 'file', action: 'write' }, requirements: { destination: { within: { root: 'write' } } }, on_unmet: { decision: 'deny' } },
    { id: 'cat', decision: 'allow', conditions: { domain: 'argv', executable: 'cat', arguments: { kind: 'any' } }, requirements: { targets: { action: 'read', all: { within: { root: 'read' } } } }, on_unmet: { decision: 'deny' } },
  );
  await writeFile(path, JSON.stringify(rules));
  await f.operator(['policy', 'review'], 'manual\nyes\n');
  await f.operator(['mcp', 'setup', '--agent', 'claude']);
  const provider = await scriptedProvider(t, [
    { name: 'Bash', input: { command: `cat '${existing}'` } },
    { name: 'Write', input: { file_path: native, content: 'ACME_NATIVE_TEMPORARY_TEXT' } },
    { name: 'Bash', input: { command: `printf ACME_SHELL_TEMPORARY_TEXT > '${shell}' && cat '${shell}'` } },
    { name: 'Read', input: { file_path: native } },
  ]);
  const executable = (await promisify(execFile)('which', ['claude'])).stdout.trim();
  const conversation = query({ prompt: 'Use the Acme temporary files.', options: {
    pathToClaudeCodeExecutable: executable, permissionMode: 'default', cwd: f.root,
    settingSources: ['project', 'local'], tools: ['Bash', 'Read', 'Write'], persistSession: false, maxTurns: 6,
    canUseTool: async name => assert.fail(`Unexpected permission prompt: ${name}`),
    env: { ...process.env, ANTHROPIC_API_KEY: 'acme-local-test-key', ANTHROPIC_BASE_URL: provider.url,
      CLAUDE_CONFIG_DIR: join(f.root, 'claude-config'), CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC: '1' },
  } });
  t.after(() => conversation.close());
  for await (const message of conversation) {
    if (message.type === 'result') assert.equal(message.subtype, 'success', JSON.stringify(message));
  }
  const output = JSON.stringify(provider.results);
  for (const value of ['ACME_EXISTING_TEMPORARY_OUTPUT', 'ACME_NATIVE_TEMPORARY_TEXT', 'ACME_SHELL_TEMPORARY_TEXT']) assert.ok(output.includes(value), output);
  assert.equal(await readFile(native, 'utf8'), 'ACME_NATIVE_TEMPORARY_TEXT');
  assert.equal(await readFile(shell, 'utf8'), 'ACME_SHELL_TEMPORARY_TEXT');
  const decisions = (await f.events()).filter(event => event.kind === 'decision');
  assert.equal(decisions.length, 4);
  assert.ok(decisions.every(event => event.details.decision === 'allow'), JSON.stringify(decisions));
});
