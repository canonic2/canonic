import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile, writeFile, access, mkdir, chmod } from 'node:fs/promises';
import { join } from 'node:path';
import { execFile } from 'node:child_process';
import { promisify } from 'node:util';
import { query } from '@anthropic-ai/claude-agent-sdk';
import { enrolledRuntime } from '../fixtures/runtime.mjs';
import { scriptedProvider } from './claude-provider.mjs';
import { preparePolicyChange } from './policy-proposal.mjs';

const exec = promisify(execFile);
const quote = s => `'${s.replaceAll("'", "'\\''")}'`;
test('Claude can cd to its current project before permitted Git inspection without approval', { timeout: 60000 }, async t => {
  const f = await enrolledRuntime(t);
  await exec('git', ['init', '-q', f.root]);
  const rulesPath = join(f.root, '.shield/rules.yml');
  const rules = await f.readRules();
  for (const [id, executable, values] of [['node-version', 'node', ['-v']], ['npm-version', 'npm', ['-v']], ['git-status', 'git', ['status', '--short']]]) {
    rules.rules.push({ id, decision: 'allow', conditions: { domain: 'argv', executable, arguments: { kind: 'exact', values } } });
  }
  await writeFile(rulesPath, JSON.stringify(rules));
  await f.operator(['policy', 'review'], 'manual\nyes\n');
  await f.operator(['mcp', 'setup', '--agent', 'claude']);
  const source = `node -v; npm -v; cd ${quote(f.root)} && git status --short`;
  const provider = await scriptedProvider(t, [{ name: 'Bash', input: { command: source } }]);
  const prompts = [];
  await host(t, f, provider, { allowedTools: [], permissionMode: 'default', canUseTool: async name => {
    prompts.push(name);
    return { behavior: 'deny', message: 'Unexpected approval.' };
  } });
  assert.deepEqual(prompts, [], JSON.stringify(provider.results));
  assert.doesNotMatch(JSON.stringify(provider.results), /hook error|approval required/i);
  const decisions = (await f.events()).filter(event => event.kind === 'decision');
  assert.equal(decisions.length, 1);
  assert.equal(decisions[0].details.decision, 'allow');
});

test('a project executable named node does not acquire syntax-check read permissions', { timeout: 60000 }, async t => {
  const f = await enrolledRuntime(t);
  await f.operator(['mcp', 'setup', '--agent', 'claude']);
  const tools = join(f.root, 'tools');
  await mkdir(tools);
  const executable = join(tools, 'node');
  const marker = join(f.root, 'impostor-executed');
  await writeFile(executable, `#!/bin/sh\nprintf executed > ${quote(marker)}\n`);
  await chmod(executable, 0o755);
  const input = { command: `${quote(executable)} --check ${quote(f.allowed)}` };
  const provider = await scriptedProvider(t, [{ name: 'Bash', input }]);
  const calls = [];
  await host(t, f, provider, { canUseTool: async (name, value) => {
    calls.push({ name, input: value });
    return { behavior: 'deny', message: 'Acme declines uninspected execution.' };
  } });
  assert.deepEqual(calls, [{ name: 'Bash', input }]);
  await assert.rejects(access(marker));
});
for (const permissionMode of ['default', 'auto', 'bypassPermissions']) {
  for (const action of ['accept', 'decline', 'cancel']) {
    test(`Claude policy confirmation in ${permissionMode} mode honors ${action}`, { timeout: 60000 }, async t => {
      const f = await enrolledRuntime(t);
      await f.operator(['mcp', 'setup', '--agent', 'claude']);
      const args = await preparePolicyChange(f.root, 'claude');
      const before = JSON.parse(await readFile(f.statePath, 'utf8'));
      const name = 'mcp__shield__policy_apply';
      const provider = await scriptedProvider(t, [{ name, input: args }]);
      const calls = [];
      const toolApprovals = [];
      await host(t, f, provider, { permissionMode, allowDangerouslySkipPermissions: true,
        canUseTool: async (name, input) => {
          toolApprovals.push({ name, input });
          return { behavior: 'deny', message: 'Policy confirmation must use the MCP form.' };
        },
        onElicitation: async request => {
          calls.push({ request, active: JSON.parse(await readFile(f.statePath, 'utf8')).active });
          return action === 'accept' ? { action, content: {} } : { action };
        },
      });
      assert.deepEqual(toolApprovals, [], 'Policy confirmation must use the MCP form, independently of ordinary tool approval.');
      assert.equal(calls.length, 1, JSON.stringify(provider.results));
      const [{ request, active }] = calls;
      assert.equal(request.serverName, 'shield');
      assert.match(request.message, /Approve this Shield policy change/);
      assert.match(request.message, /Rule ID: acme-host-approved/);
      assert.doesNotMatch(request.message, /candidate_digest|proposal_id|apply_arguments/);
      assert.deepEqual(request.requestedSchema.properties, {});
      assert.deepEqual(active, before.active);
      const after = JSON.parse(await readFile(f.statePath, 'utf8'));
      assert.equal(after.proposals.at(-1).status, action === 'accept' ? 'applied' : 'pending', JSON.stringify(provider.results));
      if (action === 'accept') assert.notEqual(after.policy_revision, before.policy_revision);
      else {
        assert.deepEqual(after.active, before.active);
        assert.equal(after.policy_revision, before.policy_revision);
      }
    });
  }
}
async function host(t, fixture, provider, permission = {}) {
  const executable = (await exec('which', ['claude'])).stdout.trim();
  const conversation = query({ prompt: 'Perform the Acme fixture tools in order.', options: {
    pathToClaudeCodeExecutable: executable, cwd: fixture.root, settingSources: ['project', 'local'],
    tools: ['Read', 'Write', 'Bash'], allowedTools: ['Read', 'Write', 'Bash'], persistSession: false, maxTurns: 8,
    ...permission,
    env: { ...process.env, ANTHROPIC_API_KEY: 'acme-local-test-key', ANTHROPIC_BASE_URL: provider.url,
      CLAUDE_CONFIG_DIR: join(fixture.root, 'claude-config'), CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC: '1' },
  } });
  t.after(() => conversation.close());
  const messages = [];
  for await (const message of conversation) messages.push(message);
  assert.ok(messages.some(m => m.type === 'result' && m.subtype === 'success'), JSON.stringify(messages.filter(m => m.type === 'result')));
}

test('Claude receives proposal context after its first native Ask', { timeout: 60000 }, async t => {
  const f = await enrolledRuntime(t);
  await f.operator(['mcp', 'setup', '--agent', 'claude']);
  const input = { command: 'npm --version' };
  const provider = await scriptedProvider(t, [{ name: 'Bash', input }]);
  const calls = [];
  await host(t, f, provider, { canUseTool: async (name, value) => {
    calls.push({ name, input: value });
    return { behavior: 'allow', updatedInput: value };
  } });
  assert.deepEqual(calls, [{ name: 'Bash', input }]);
  // Inspect what the real host sends to the model, not just the hook's JSON.
  const context = JSON.stringify(provider.requests.at(-1).messages);
  assert.match(context, /Shield: policy asks for approval for this request/);
  assert.match(context, /shield_docs_search/);
  assert.match(context, /shield_docs_read/);
  assert.match(context, /policy_propose/);
  assert.match(context, /policy_apply/);
  assert.match(context, /source_entry_id/);
  assert.doesNotMatch(context, /policy_propose with change|rule_add/);
  assert.doesNotMatch(context, /Allow this request\?/);
  // Delivering guidance neither creates a proposal nor activates policy.
  const after = JSON.parse(await readFile(f.statePath, 'utf8'));
  const before = JSON.parse(f.state);
  assert.deepEqual(after.proposals, before.proposals);
  assert.deepEqual(after.active, before.active);
});

test('installed Claude command hooks discover native reads and withhold unanswered Ask/Deny', { timeout: 60000 }, async t => {
  const f = await enrolledRuntime(t);
  await f.operator(['mcp', 'setup', '--agent', 'claude']);
  const provider = await scriptedProvider(t, [
    { name: 'Read', input: { file_path: f.allowed } },
    { name: 'Read', input: { file_path: f.denied } },
    { name: 'Read', input: { file_path: f.asked } },
  ]);
  await host(t, f, provider);
  assert.ok(provider.results.some(r => JSON.stringify(r).includes('ACME_ALLOWED_TEXT')));
  assert.ok(provider.results.some(r => JSON.stringify(r).includes('Shield: blocked. The active policy does not allow this request')));
  assert.match(JSON.stringify(provider.requests), /Shield: denied/);
  assert.doesNotMatch(JSON.stringify(provider.results), /policy_propose|source_entry_id|apply_arguments/);
  assert.ok(!provider.results.some(r => /ACME_DENIED_TEXT|ACME_ASKED_TEXT/.test(JSON.stringify(r))));
  const events = await f.events();
  assert.ok(events.some(e => e.kind === 'decision' && e.provenance.agent === 'claude-code'));
  assert.ok(events.some(e => e.kind === 'completion'));
  assert.doesNotMatch(JSON.stringify(events), /ACME_ALLOWED_TEXT|ACME_DENIED_TEXT/);
  assert.equal((await f.cli(['status'])).coverage.live_host_verification, 'unverified');
});

for (const behavior of ['allow', 'deny']) {
  test(`Claude native Ask reaches host approval and honors ${behavior}`, { timeout: 60000 }, async t => {
    const f = await enrolledRuntime(t);
    await f.operator(['mcp', 'setup', '--agent', 'claude']);
    const marker = join(f.root, 'approval-side-effect');
    const command = `touch ${quote(marker)}`;
    const calls = [];
    const provider = await scriptedProvider(t, [
      { name: 'Bash', input: { command } },
      { name: 'Read', input: { file_path: f.asked } },
      { name: 'Read', input: { file_path: f.denied } },
    ]);
    await host(t, f, provider, { canUseTool: async (name, input) => {
      calls.push({ name, input });
      if (name === 'Bash') await assert.rejects(access(marker));
      return behavior === 'allow' ? { behavior, updatedInput: input } : { behavior, message: 'Acme fixture declined.' };
    } });
    assert.deepEqual(calls, [
      { name: 'Bash', input: { command } },
      { name: 'Read', input: { file_path: f.asked } },
    ]);
    if (behavior === 'allow') await access(marker);
    else await assert.rejects(access(marker));
    const results = JSON.stringify(provider.results);
    assert.doesNotMatch(results, /ACME_DENIED_TEXT/);
    if (behavior === 'allow') assert.match(results, /ACME_ASKED_TEXT/);
    else assert.doesNotMatch(results, /ACME_ASKED_TEXT/);
    const events = await f.events();
    assert.ok(events.some(e => e.kind === 'decision' && e.details.decision === 'ask'
      && e.details.original_input.command === command && e.details.policy_revision));
  });
}

test('installed Claude native hooks deny shell and write before side effects', { timeout: 60000 }, async t => {
  const f = await enrolledRuntime(t);
  await f.operator(['mcp', 'setup', '--agent', 'claude']);
  const marker = join(f.root, 'side-effect');
  const provider = await scriptedProvider(t, [
    { name: 'Bash', input: { command: `touch ${quote(marker)}` } },
    { name: 'Write', input: { file_path: marker, content: 'Acme' } },
  ]);
  await host(t, f, provider);
  await assert.rejects(access(marker));
  assert.ok(provider.results.some(r => JSON.stringify(r).includes('Shield:')));
});

test('installed Claude hooks allow Git staging and deny destructive Git policy', { timeout: 60000 }, async t => {
  const f = await enrolledRuntime(t);
  await exec('git', ['init', '-q', f.root]);
  const rulesPath = join(f.root, '.shield/rules.yml');
  const rules = await f.readRules();
  for (const [id, decision, values] of [['stage', 'allow', ['add', 'allowed.txt']], ['destructive', 'deny', ['reset', '--hard']]]) {
    rules.rules.push({ id, decision, reason: `Acme ${id}`,
      conditions: { domain: 'argv', executable: 'git', arguments: { kind: 'exact', values } } });
  }
  await writeFile(rulesPath, JSON.stringify(rules));
  await f.operator(['policy', 'review'], 'manual\nyes\n');
  await f.operator(['mcp', 'setup', '--agent', 'claude']);
  const provider = await scriptedProvider(t, [
    { name: 'Bash', input: { command: 'git add allowed.txt' } },
    { name: 'Bash', input: { command: 'git reset --hard' } },
  ]);
  await host(t, f, provider);
  assert.equal((await exec('git', ['diff', '--cached', '--name-only'], { cwd: f.root })).stdout.trim(), 'allowed.txt');
  assert.ok(provider.results.some(r => JSON.stringify(r).includes('Shield: blocked. The active policy does not allow this request')));
  assert.match(JSON.stringify(provider.requests), /Shield: denied/);
  assert.doesNotMatch(JSON.stringify(provider.results), /policy_propose|source_entry_id|apply_arguments/);
  assert.ok((await f.events()).some(e => e.kind === 'decision' && e.provenance.agent === 'claude-code' && e.details.decision === 'allow'));
});

test('native wrapper explicitly denies an evaluator startup failure', { timeout: 60000 }, async t => {
  const f = await enrolledRuntime(t);
  await f.operator(['mcp', 'setup', '--agent', 'claude']);
  const settingsPath = join(f.root, '.claude/settings.local.json');
  const settings = JSON.parse(await readFile(settingsPath, 'utf8'));
  settings.hooks.PreToolUse[0].hooks[0].command = `${quote(join(f.root, 'missing-shield'))} || exit 2`;
  await writeFile(settingsPath, JSON.stringify(settings));
  const marker = join(f.root, 'failure-side-effect');
  const provider = await scriptedProvider(t, [{ name: 'Bash', input: { command: `touch ${quote(marker)}` } }]);
  await host(t, f, provider);
  await assert.rejects(access(marker));
  assert.equal((await f.cli(['doctor'])).repair_needed, true);
});

test('Claude host timeout can continue execution despite a configured policy hook', { timeout: 60000 }, async t => {
  const f = await enrolledRuntime(t);
  await f.operator(['mcp', 'setup', '--agent', 'claude']);
  const settingsPath = join(f.root, '.claude/settings.local.json');
  const settings = JSON.parse(await readFile(settingsPath, 'utf8'));
  settings.hooks.PreToolUse[0].hooks[0] = { type: 'command', command: 'sleep 3', timeout: 1 };
  await writeFile(settingsPath, JSON.stringify(settings));
  const marker = join(f.root, 'timeout-side-effect');
  const provider = await scriptedProvider(t, [{ name: 'Bash', input: { command: `touch ${quote(marker)}` } }]);
  await host(t, f, provider);
  await access(marker); // Documents the host boundary; this is not a protection pass.
});
