import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile, writeFile, access, rm } from 'node:fs/promises';
import { join } from 'node:path';
import { query } from '@anthropic-ai/claude-agent-sdk';
import { createClaudeReadOnlyHooks } from '../src/modules/claude-read-only/index.mjs';
import { binary, enrolledRuntime, authorizationState } from '../fixtures/runtime.mjs';
import { scriptedProvider } from './claude-provider.mjs';

const quote = value => `'${value.replaceAll("'", "'\\''")}'`;

test('native Claude retries an operator-approved Ask read exactly once', { timeout: 60000 }, async t => {
  const fixture = await enrolledRuntime(t);
  const hooks = await createClaudeReadOnlyHooks({ binary, project: fixture.root });
  const seen = [];
  const pre = hooks.PreToolUse[0].hooks[0];
  const observed = { ...hooks, PreToolUse: [{ hooks: [async (...args) => {
    const value = await pre(...args);
    seen.push({ id: args[0].tool_use_id, decision: value.hookSpecificOutput.permissionDecision });
    return value;
  }] }] };
  let grant;
  const provider = await scriptedProvider(t, [
    { name: 'Read', input: { file_path: fixture.asked } },
    { name: 'Read', input: { file_path: fixture.asked }, before: async () => {
      const rows = (await fixture.cli(['request', 'list'])).requests;
      assert.equal(rows.length, 1);
      await fixture.operator(['request', 'approve', rows[0].request_id]);
      const approved = await fixture.cli(['request', 'show', rows[0].request_id]);
      assert.equal(approved.status, 'approved');
      assert.ok(approved.grant.approval_id);
      grant = { request_id: approved.request_id, approval_id: approved.grant.approval_id };
    } },
    { name: 'Read', input: { file_path: fixture.asked } },
  ]);
  await host(t, fixture, provider, observed);
  assert.deepEqual(seen.map(v => v.decision), ['deny', 'allow', 'deny']);
  assert.equal(new Set(seen.map(v => v.id)).size, 3);
  assert.equal(new Set(provider.results.filter(v => JSON.stringify(v).includes('ACME_ASKED_TEXT')).map(v => v.tool_use_id)).size, 1);
  await assert.rejects(fixture.cli(['request', 'show', grant.request_id]));
  const events = (await fixture.events());
  assert.equal(events.filter(e => e.kind === 'read_grant_consumed').length, 1);
  const granted = events.find(e => e.kind === 'decision' && e.details.host_decision === 'allow');
  assert.equal(granted.details.decision, 'ask');
  assert.equal(granted.links.approval_id, grant.approval_id);
  const completed = events.filter(e => e.kind === 'completion');
  assert.equal(completed.length, 1);
  assert.equal(completed[0].links.decision_event_id, granted.event_id);
  assert.doesNotMatch(JSON.stringify(events), /ACME_ASKED_TEXT/);
});

test('Claude setup preapproves Shield MCP without native hooks or SDK permission overrides', { timeout: 60000 }, async t => {
  const fixture = await enrolledRuntime(t);
  await fixture.operator(['mcp', 'setup', '--agent', 'claude']);
  const before = JSON.parse(await readFile(fixture.statePath, 'utf8'));
  const path = join(fixture.root, '.claude/settings.local.json');
  const settings = JSON.parse(await readFile(path, 'utf8'));
  settings.disableAllHooks = true;
  await writeFile(path, JSON.stringify(settings));
  const approvals = [];
  const provider = await scriptedProvider(t, [
    { name: 'mcp__shield__status', input: {} },
    { name: 'mcp__shield__policy_propose', input: { change: { kind: 'rule_add', rule: {
      id: 'acme-proposed-read', decision: 'allow', reason: 'Acme inspection proposal',
      conditions: { domain: 'argv', executable: 'acme', arguments: { kind: 'exact', values: ['inspect'] } },
    } }, idempotency_key: 'acme-claude-approval' } },
  ]);
  const conversation = query({ prompt: 'Inspect the Acme project status.', options: {
    cwd: fixture.root, settingSources: ['project', 'local'], tools: [], persistSession: false,
    maxTurns: 4, canUseTool: async name => {
      approvals.push(name);
      return { behavior: 'deny', message: 'Acme fixture declines unexpected approval requests.' };
    },
    env: { ...process.env, ANTHROPIC_API_KEY: 'acme-local-test-key', ANTHROPIC_BASE_URL: provider.url,
      CLAUDE_CONFIG_DIR: join(fixture.root, 'claude-config'), CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC: '1' },
  } });
  t.after(() => conversation.close());
  const messages = [];
  for await (const message of conversation) messages.push(message);
  assert.ok(messages.some(message => message.type === 'result' && message.subtype === 'success'));
  assert.ok(messages.some(message => message.type === 'system' && message.subtype === 'init'
    && message.mcp_servers?.some(server => server.name === 'shield' && server.status === 'connected')));
  assert.ok(provider.results.some(result => result.tool_use_id && JSON.stringify(result).includes('project_id')));
  assert.deepEqual(approvals, []);
  const offered = provider.requests[0].tools.map(tool => tool.name);
  for (const name of ['pack_list', 'rule_list', 'policy_show', 'request_list', 'history_list', 'policy_propose']) {
    assert.ok(offered.includes(`mcp__shield__${name}`), `Claude omitted ${name}: ${JSON.stringify(offered)}`);
  }
  const after = JSON.parse(await readFile(fixture.statePath, 'utf8'));
  assert.equal(after.proposals.length, before.proposals.length + 1, JSON.stringify(provider.results));
  assert.equal(after.proposals.at(-1).status, 'pending');
  assert.deepEqual(after.active, before.active);
  assert.deepEqual(after.candidates, before.candidates);
  delete settings.disableAllHooks;
  await writeFile(path, JSON.stringify(settings));
  const status = await fixture.cli(['status']);
  assert.equal(status.coverage.registered_integrations[0].status, 'registered');
  assert.equal(status.coverage.live_host_verification, 'unverified');
  assert.equal(status.coverage.installed_integrations[0].surface, 'native-hooks');
});
async function host(t, fixture, provider, hooks, signal) {
  const conversation = query({ prompt: 'Perform the Acme fixture tools in order.', options: {
    cwd: fixture.root, hooks, abortController: signal, settingSources: [], strictMcpConfig: true, mcpServers: {},
    tools: ['Read', 'Write', 'Bash'], allowedTools: ['Read', 'Write', 'Bash'], persistSession: false,
    maxTurns: 12, systemPrompt: 'Acme local interoperability fixture.',
    env: { ...process.env, ANTHROPIC_API_KEY: 'acme-local-test-key', ANTHROPIC_BASE_URL: provider.url,
      CLAUDE_CONFIG_DIR: join(fixture.root, 'claude-config'), CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC: '1' },
  } });
  t.after(() => conversation.close());
  const messages = [];
  for await (const message of conversation) messages.push(message);
  assert.ok(messages.some(message => message.type === 'result' && message.subtype === 'success'));
  return messages;
}

test('real Claude SDK discovers the callback and enforces read decisions and operator/state blocks', { timeout: 60000 }, async t => {
  const fixture = await enrolledRuntime(t);
  await writeFile(fixture.allowed, 'ACME_FIRST_LINE\nACME_ALLOWED_TEXT\n');
  const seen = [];
  const hooks = await createClaudeReadOnlyHooks({ binary, project: fixture.root });
  const callback = hooks.PreToolUse[0].hooks[0];
  const observed = { ...hooks, PreToolUse: [{ timeout: hooks.PreToolUse[0].timeout, hooks: [async (...args) => {
    const result = await callback(...args);
    seen.push({ name: args[0].tool_name, fields: Object.keys(args[0]),
      toolFields: Object.keys(args[0].tool_input), sameId: args[0].tool_use_id === args[1], result });
    return { ...result, systemMessage: 'ACME_HISTORY_DIAGNOSTIC' };
  }] }] };
  const provider = await scriptedProvider(t, [
    { name: 'Read', input: { file_path: fixture.allowed, offset: 2, limit: 1 } },
    { name: 'Read', input: { file_path: fixture.asked } },
    { name: 'Read', input: { file_path: fixture.denied } },
    { name: 'Write', input: { file_path: fixture.statePath, content: 'tampered' } },
    { name: 'Bash', input: { command: `printf compromised > ${quote(fixture.statePath)}` } },
    { name: 'Bash', input: { command: `${quote(binary)} init --project ${quote(fixture.root)}` } },
  ]);
  const messages = await host(t, fixture, provider, observed);
  assert.ok(messages.some(message => JSON.stringify(message).includes('ACME_HISTORY_DIAGNOSTIC')),
    `native message types: ${messages.map(message => message.type).join(', ')}`);
  assert.deepEqual(seen.map(value => value.result.hookSpecificOutput.permissionDecision), ['allow', 'deny', 'deny', 'deny', 'deny', 'deny'],
    JSON.stringify(seen));
  const text = JSON.stringify(provider.results);
  assert.match(text, /ACME_ALLOWED_TEXT/);
  assert.doesNotMatch(text, /ACME_FIRST_LINE|ACME_ASKED_TEXT|ACME_DENIED_TEXT/);
  assert.match(text, /approval required for read read-request-/);
  assert.match(text, /shield request show read-request-/);
  assert.deepEqual(authorizationState(await readFile(fixture.statePath)), authorizationState(fixture.state));
  const events = (await fixture.events());
  const decisions = events.filter(e => e.kind === 'decision');
  assert.equal(decisions.length, 6);
  assert.deepEqual(decisions.map(e => e.details.decision), ['allow', 'ask', 'deny', null, null, null]);
  const completion = events.filter(e => e.kind === 'completion');
  assert.equal(completion.length, 1);
  assert.equal(completion[0].details.status, 'completed');
  assert.equal(completion[0].details.exit_code, null);
  assert.equal(completion[0].links.decision_event_id, decisions[0].event_id);
  assert.equal(completion[0].provenance.host_event_name, 'PostToolUse');
  assert.equal(completion[0].provenance.sources.session_id, 'host');
  assert.doesNotMatch(JSON.stringify(events), /ACME_ALLOWED_TEXT|ACME_FIRST_LINE|compromised|tampered/);
});

test('real Claude SDK records failure after an allowed read without storing error payloads', { timeout: 30000 }, async t => {
  const fixture = await enrolledRuntime(t);
  const hooks = await createClaudeReadOnlyHooks({ binary, project: fixture.root });
  const pre = hooks.PreToolUse[0].hooks[0];
  const actual = { ...hooks, PreToolUse: [{ hooks: [async (...args) => {
    const result = await pre(...args);
    assert.equal(result.hookSpecificOutput.permissionDecision, 'allow');
    await rm(fixture.allowed);
    return result;
  }] }] };
  const provider = await scriptedProvider(t, [{ name: 'Read', input: { file_path: fixture.allowed } }]);
  await host(t, fixture, provider, actual);
  const events = (await fixture.events());
  const completion = events.filter(e => e.kind === 'completion');
  assert.equal(completion.length, 1);
  assert.equal(completion[0].details.status, 'failed');
  assert.equal(completion[0].provenance.host_event_name, 'PostToolUseFailure');
  assert.equal(completion[0].links.decision_event_id, events.find(e => e.kind === 'decision').event_id);
  assert.equal(completion[0].details.exit_code, null);
  assert.doesNotMatch(JSON.stringify(events), /File does not exist|ENOENT|ACME_ALLOWED_TEXT/);
});

test('real Claude SDK callback timeout blocks an otherwise permitted write command', { timeout: 30000 }, async t => {
  const fixture = await enrolledRuntime(t);
  const marker = join(fixture.root, 'unexpected-execution');
  let invoked = false;
  const hooks = { PreToolUse: [{ timeout: 1, hooks: [async () => { invoked = true; return new Promise(() => {}); }] }] };
  const provider = await scriptedProvider(t, [{ name: 'Bash', input: { command: `printf tampered > ${quote(marker)}` } }]);
  await host(t, fixture, provider, hooks);
  assert.ok(invoked);
  await assert.rejects(access(marker));
  assert.match(JSON.stringify(provider.results), /timed? ?out|timeout/i);
});
