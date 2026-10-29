import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtemp, writeFile, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { createHash } from 'node:crypto';
import { Client } from '@modelcontextprotocol/sdk/client/index.js';
import { StdioClientTransport } from '@modelcontextprotocol/sdk/client/stdio.js';

import { initialize, reviewProposal, historySettings, operator } from '../fixtures/runtime.mjs';

const binary = fileURLToPath(new URL('../../target/release/shield', import.meta.url));
const rule = { id: 'acme-read', decision: 'allow', reason: 'Read Acme data',
  conditions: { domain: 'argv', executable: 'acme', arguments: { kind: 'exact', values: ['read'] } } };
const request = root => ({ input: { kind: 'argv', executable: 'acme', arguments: ['read'], original_input: { command: 'acme read' } }, context: { cwd: root, environment: {} } });
const data = response => response.structuredContent.data;


test('MCP proposal, CLI review, MCP receipt, reconnect and history share durable context', { timeout: 60000 }, async t => {
  const root = await mkdtemp(join(tmpdir(), 'shield-review-sdk-'));
  t.after(() => rm(root, { recursive: true, force: true }));
  const config = join(root, 'config.json');
  await writeFile(config, JSON.stringify({ schema_version: 1, packs: [] }));
  await initialize(root, config);
  const connect = async () => {
    const client = new Client({ name: 'acme-sdk', version: '1' });
    const transport = new StdioClientTransport({ command: binary,
      args: ['mcp', 'serve', '--project', root, '--agent', 'codex', '--session-id', 'acme-session'], stderr: 'pipe' });
    await client.connect(transport, { timeout: 15000 });
    t.after(() => client.close());
    return client;
  };
  let client = await connect();
  const call = async (name, args) => client.callTool({ name, arguments: args });
  const tools = await client.listTools();
  assert.ok(tools.tools.some(tool => tool.name === 'policy_propose'));
  assert.equal(tools.tools.some(tool => tool.name.includes('approve')), false);
  const args = { change: { kind: 'rule_add', rule }, idempotency_key: 'acme-one', agent_reason: 'Inspect Acme' };
  const proposed = data(await call('policy_propose', args));
  assert.equal(proposed.status, 'pending');
  assert.equal(proposed.provenance.session_id, 'acme-session');
  assert.equal(proposed.provenance.agent, 'codex');
  assert.equal(proposed.provenance.host_version, '1');
  assert.ok(proposed.provenance.connection_id.startsWith('connection-'));
  assert.equal((await call('policy_receipt', { proposal_id: proposed.proposal_id })).structuredContent.error.code, 'operator_review_required');
  assert.equal((await call('policy_propose', { ...args, approved: true })).isError, true);
  assert.equal((await call('history_list', { project: '/another/project' })).isError, true);
  const listed = data(await call('policy_show', { session_id: 'acme-session' }));
  assert.equal(listed.proposals[0].proposal_id, proposed.proposal_id);
  await reviewProposal(root, proposed.proposal_id);
  const approved = data(await call('policy_show', { proposal_id: proposed.proposal_id }));
  assert.equal(approved.status, 'applied');
  assert.equal(approved.operator.operator_id, null);
  assert.equal(approved.operator.mechanism, 'operator-cli');
  assert.equal(data(await call('policy_receipt', { proposal_id: proposed.proposal_id })).status, 'applied');
  const decision = data(await call('policy_evaluate', request(root)));
  assert.equal(decision.decision, 'allow');
  assert.equal(decision.disposition, 'preview');
  await assert.rejects(() => call('shield_record_completion', { request_id: 'fake' }), {
    code: -32602, message: 'MCP error -32602: Unknown tool',
  });
  assert.deepEqual(data(await call('history_list', {})).entries, [], 'diagnostics and configuration changes create no command entries');
  assert.equal(approved.provenance.session_id, 'acme-session');
  await client.close();
  client = await connect();
  const retry = data(await call('policy_propose', args));
  assert.equal(retry.proposal_id, proposed.proposal_id);
  assert.equal(retry.provenance.connection_id, proposed.provenance.connection_id);
  assert.equal(data(await call('policy_receipt', { proposal_id: proposed.proposal_id })).status, 'applied');
  const removal = data(await call('policy_propose', { change: { kind: 'rule_remove', rule_id: rule.id }, idempotency_key: 'acme-remove' }));
  await reviewProposal(root, removal.proposal_id, 'reject');
  assert.equal((await call('policy_receipt', { proposal_id: removal.proposal_id })).isError, true);
});

test('active MCP diagnostics reload only operator activation and retain authorization after purge', { timeout: 60000 }, async t => {
  const root = await mkdtemp(join(tmpdir(), 'shield-runtime-sdk-'));
  t.after(() => rm(root, { recursive: true, force: true }));
  const config = join(root, 'config.json');
  await writeFile(config, JSON.stringify({ schema_version: 1, packs: [] }));
  await initialize(root, config);
  const client = new Client({ name: 'acme-runtime', version: '1' });
  await client.connect(new StdioClientTransport({ command: binary,
    args: ['mcp', 'serve', '--project', root, '--agent', 'codex'], stderr: 'pipe' }));
  t.after(() => client.close());
  const call = async (name, args = {}) => client.callTool({ name, arguments: args });
  const check = async () => call('policy_evaluate', request(root));
  assert.equal(data(await check()).decision, 'ask');
  const first = data(await call('status')).active;
  assert.ok(first.activation_id);
  assert.equal((await call('status', { project: '/other' })).isError, true);
  await assert.rejects(() => call('shield_runtime_activate'), {
    code: -32602, message: 'MCP error -32602: Unknown tool',
  });
  assert.equal((await client.listTools()).tools.some(tool => tool.name.includes('activate')), false);
  const proposed = data(await call('policy_propose', { change: { kind: 'rule_add', rule }, idempotency_key: 'active' }));
  assert.equal((await call('policy_receipt', { proposal_id: proposed.proposal_id })).isError, true);
  assert.equal(data(await check()).decision, 'ask');
  await reviewProposal(root, proposed.proposal_id);
  const second = data(await call('status')).active;
  assert.ok(second.activation_id);
  await call('policy_receipt', { proposal_id: proposed.proposal_id });
  assert.notEqual(first.activation_id, second.activation_id);
  await historySettings(root, false, true);
  const result = data(await check());
  assert.equal(result.decision, 'allow');
  assert.equal(data(await call('status')).active.activation_id, second.activation_id);
  assert.equal(result.disposition, 'preview');
  assert.equal(data(await call('status')).active.activation_id, second.activation_id);
});

test('pack proposals retain acquired bytes and instance-qualified rules for offline review', { timeout: 60000 }, async t => {
  const root = await mkdtemp(join(tmpdir(), 'shield-pack-sdk-'));
  const source = await mkdtemp(join(tmpdir(), 'shield-registry-sdk-'));
  t.after(() => Promise.all([root, source].map(path => rm(path, { recursive: true, force: true }))));
  const config = join(root, 'config.json');
  await writeFile(config, JSON.stringify({ schema_version: 1, packs: [] }));
  await initialize(root, config);
  const pack = JSON.stringify({ schema_version: 1, kind: 'pack', id: 'acme/read', version: '0.1.0', description: 'Acme fixture', license: 'MIT', maintainers: ['Acme'], engines: { shield: '*' }, capabilities: ['structured-rules-v1', 'argument-rules-v1'], rules: [{ id: rule.id, conditions: { domain: 'argv', executable: 'acme', arguments: { kind: 'exact', values: ['read'] } }, reason: 'Acme pack rule', decision: { loose: 'allow', recommended: 'ask', strict: 'deny' } }] });
  await writeFile(join(source, 'pack.json'), pack);
  await writeFile(join(source, 'catalog.json'), JSON.stringify({ schema_version: 1, packs: [{ id: 'acme/read', version: '0.1.0', path: 'pack.json', sha256: createHash('sha256').update(pack).digest('hex') }] }));
  await operator(root, ['registry', 'add', 'acme', source]);
  const client = new Client({ name: 'acme-packs', version: '1' });
  await client.connect(new StdioClientTransport({ command: binary, args: ['mcp', 'serve', '--project', root], stderr: 'pipe' }));
  t.after(() => client.close());
  const call = async (name, args = {}) => client.callTool({ name, arguments: args });
  const args = { change: { kind: 'pack_add', reference: 'acme/read@0.1.0' }, idempotency_key: 'pack-one' };
  const info = data(await call('pack_info', { reference: args.change.reference }));
  assert.equal(info.compatibility.supported, true);
  assert.equal(info.rule_count, 1);
  assert.equal(info.rules, undefined);
  const detail = data(await call('pack_info', { reference: args.change.reference, rule_id: rule.id }));
  assert.equal(detail.rule.conditions.domain, 'argv');
  assert.deepEqual(detail.rule.profile_decisions, { loose: 'allow', recommended: 'ask', strict: 'deny' });
  const proposed = data(await call('policy_propose', args));
  assert.equal(proposed.status, 'pending');
  assert.deepEqual(data(await call('pack_list')).packs, []);
  await rm(source, { recursive: true, force: true });
  assert.equal(data(await call('policy_propose', args)).proposal_id, proposed.proposal_id);
  await reviewProposal(root, proposed.proposal_id);
  const installed = data(await call('pack_list')).packs[0];
  assert.equal(installed.id, 'acme/read');
  assert.ok(installed.selector.startsWith('pack:'));
  const packRule = data(await call('rule_list')).rules[0];
  assert.deepEqual(packRule.rule_ref, { scope: 'pack', pack_selector: installed.selector, rule_id: rule.id });
  const inspectedPack = data(await call('rule_list', { rule_ref: packRule.rule_ref }));
  assert.deepEqual(inspectedPack.decision, { loose: 'allow', recommended: 'ask', strict: 'deny' });
  assert.equal(inspectedPack.effective_decision, 'ask');
  const custom = data(await call('policy_propose', { change: { kind: 'rule_add', rule }, idempotency_key: 'custom' }));
  await reviewProposal(root, custom.proposal_id);
  await operator(root, ['rule', 'edit', rule.id], '\n\n\n\nA revised Acme reason\nyes\n');
  assert.equal(data(await call('rule_list', { rule_ref: { scope: 'custom', rule_id: rule.id } })).decision, 'allow');
  assert.equal(data(await call('rule_list', { rule_ref: { scope: 'custom', rule_id: rule.id } })).reason, 'A revised Acme reason');
  assert.equal(data(await call('rule_list', { rule_ref: packRule.rule_ref })).effective_decision, 'ask');
  const removal = data(await call('policy_propose', { change: { kind: 'pack_remove', reference: installed.selector }, idempotency_key: 'remove-pack' }));
  await reviewProposal(root, removal.proposal_id);
  assert.equal((await call('rule_list', { rule_ref: packRule.rule_ref })).structuredContent.error.code, 'not_found');
});
