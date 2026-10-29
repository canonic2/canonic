import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtemp, writeFile, rm, realpath, readFile, access } from 'node:fs/promises';
import { execFile } from 'node:child_process';
import { promisify } from 'node:util';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { Client } from '@modelcontextprotocol/sdk/client/index.js';
import { StdioClientTransport } from '@modelcontextprotocol/sdk/client/stdio.js';

import { initialize } from '../fixtures/runtime.mjs';

const normalized = (root, executable = 'acme', args = ['read']) => ({ input: { kind: 'argv', executable, arguments: args, original_input: { executable, arguments: args } }, context: { cwd: root, environment: {} } });
const shell = (root, source, dialect) => ({ input: { kind: 'shell', source, dialect, dialect_version: null, original_input: { command: source } }, context: { cwd: root, environment: {} } });
const data = response => response.structuredContent.data;
const binary = fileURLToPath(new URL('../../target/release/shield', import.meta.url));
async function fixture(t, decision = 'allow', options = {}) {
  const root = await mkdtemp(join(tmpdir(), 'shield-sdk-'));
  t.after(() => rm(root, { recursive: true, force: true }));
  const config = join(root, 'config.json');
  const packs = [];
  for (let pack = 0; pack < (options.dense ? 32 : 0); pack++) {
    const id = `acme/dense${pack}`;
    const path = `pack${pack}.json`;
    const rules = Array.from({ length: 1000 }, (_, index) => ({ id: `r${index}`,
      conditions: { domain: 'argv', executable: 'heavy', arguments: { kind: 'any' } },
      reason: 'Acme dense evaluation fixture',
      decision: { loose: 'allow', recommended: 'allow', strict: 'allow' } }));
    await writeFile(join(root, path), JSON.stringify({ schema_version: 1, kind: 'pack', id,
      version: '0.1.0', description: 'Acme SDK dense fixture', license: 'MIT',
      maintainers: ['Acme'], engines: { shield: '*' }, capabilities: ['structured-rules-v1', 'argument-rules-v1'], rules }));
    packs.push({ id, version: '0.1.0', profile: 'recommended', path });
  }
  await writeFile(config, JSON.stringify({ schema_version: 1, packs, rules: 'rules.json' }));
  await writeFile(join(root, 'rules.json'), JSON.stringify({ schema_version: 1, kind: 'custom', capabilities: ['structured-rules-v1', 'argument-rules-v1', ...(options.nodeRead ? ['location-v1'] : [])], rules: [
    ...(options.nodeRead ? [{ id: 'syntax-read', decision: 'allow', conditions: { domain: 'file', action: 'read', sources: { all: { exact: { root: 'absolute', path: join(root, 'source.js') } } } } }] : []),
    { id: 'read', decision, reason: 'Acme SDK preview',
      conditions: { domain: 'argv', executable: 'acme', arguments: { kind: 'exact', values: ['read'] } } },
  ] }));
  const client = new Client({ name: 'acme-sdk-test', version: '1' });
  await initialize(root, config);
  const transport = new StdioClientTransport({ command: binary,
    args: ['mcp', 'serve', '--project', root,
      '--request-timeout-ms', String(options.deadline ?? 5000)], stderr: 'pipe' });
  t.after(() => client.close());
  await client.connect(transport, { timeout: 15000 });
  return { client, root, transport };
}

test('known NODE_OPTIONS preloads retain execution review for Node syntax checks', async t => {
  const { client, root } = await fixture(t, 'allow', { nodeRead: true });
  const marker = join(root, 'preload-marker');
  await writeFile(join(root, 'source.js'), "throw new Error('source must not execute');\n");
  await writeFile(join(root, 'preload.cjs'), `require('node:fs').writeFileSync(${JSON.stringify(marker)}, 'preloaded');\n`);
  for (const options of ['', '--no-warnings', '--require=./preload.cjs', '-r ./preload.cjs', '-r./preload.cjs', '"--require=./preload.cjs"', '--import=./preload.cjs', '--loader=./preload.cjs', '--experimental-loader=./preload.cjs', '--experimental_loader=./preload.cjs']) {
    const args = shell(root, 'node --check source.js', 'posix');
    args.context.environment = { NODE_OPTIONS: options };
    const result = await client.callTool({ name: 'policy_evaluate', arguments: args });
    assert.equal(result.isError, false);
    assert.equal(data(result).decision, ['', '--no-warnings'].includes(options) ? 'allow' : 'ask', options);
  }
  await assert.rejects(access(marker));
  for (const options of ['--require=./preload.cjs', '--import=./preload.cjs']) {
    await promisify(execFile)(process.execPath, ['--check', 'source.js'], { cwd: root, env: { ...process.env, NODE_OPTIONS: options } });
    assert.equal(await readFile(marker, 'utf8'), 'preloaded');
    await rm(marker);
  }
});

test('official SDK negotiates lifecycle and validates all decisions with strict canonical tools', async t => {
  for (const decision of ['allow', 'ask', 'deny']) {
    const { client, root } = await fixture(t, decision);
    assert.equal(client.getServerVersion().name, 'shield');
    await client.ping();
    const tools = await client.listTools();
    assert.equal(tools.tools.length, 20);
    assert.ok(tools.tools.every(tool => tool.inputSchema && tool.outputSchema));
    assert.ok(!tools.tools.some(tool => tool.name.startsWith('shield_preview')));
    const report = await client.callTool({ name: 'policy_evaluate', arguments: normalized(root) });
    assert.equal(report.isError, false);
    assert.equal(data(report).decision, decision);
    assert.equal(data(report).disposition, 'preview');
    assert.deepEqual(JSON.parse(report.content[1].text).data, report.structuredContent.data);
    assert.match(report.content[0].text, /preview|records/);
    if (decision === 'allow') {
      const braces = await client.callTool({ name: 'policy_evaluate', arguments: shell(root, 'acme re{a..a}d', 'bash') });
      assert.equal(braces.isError, false);
      assert.equal(data(braces).decision, 'ask');
      assert.ok(data(braces).analysis_coverage.operations.some(operation =>
        operation.coverage === 'possible-brace-expansion'));
      assert.ok(data(braces).analysis_coverage.operations.some(operation =>
        operation.evaluation?.decision === 'allow'));
    }
    for (const [source, complete] of [
      ['for part in other read; do :; done; acme "$part"', false],
      ['part=other; f() { part=read; return; part=other; }; f; acme "$part"', false],
      ['for part in {read,read}; do acme "$part"; done', true],
    ]) {
      const structured = await client.callTool({ name: 'policy_evaluate', arguments: shell(root, source, 'bash') });
      assert.equal(structured.isError, false);
      assert.equal(data(structured).decision, complete || decision === 'deny' ? decision : 'ask', source);
    }
  }
});

test('server deadline interrupts dense evaluation while the connection stays responsive', async t => {
  const { client, root } = await fixture(t, 'allow', { dense: true, deadline: 1 });
  await assert.rejects(client.callTool({ name: 'policy_evaluate', arguments: normalized(root, 'heavy', []) }, undefined, { timeout: 5000 }), error => error.code === -32001);
  await client.ping();
});

test('official SDK shell previews preserve decisions and reject context claims', async t => {
  for (const decision of ['allow', 'ask', 'deny']) {
    const { client, root } = await fixture(t, decision);
    const report = await client.callTool({ name: 'policy_evaluate', arguments: shell(root, 'acme read; (acme read)', 'posix') });
    assert.equal(report.isError, false);
    assert.equal(report.structuredContent.schema_version, 1);
    assert.equal(data(report).decision, decision);
    assert.equal(data(report).analysis_coverage.operations.length, 2);
    assert.equal(data(report).analysis_coverage.operations[0].request.cwd, await realpath(root));
    assert.deepEqual(JSON.parse(report.content[1].text).data, report.structuredContent.data);
    assert.match(report.content[0].text, /preview|records/);
    assert.equal((await client.callTool({ name: 'policy_evaluate', arguments: { ...shell(root, 'acme read', 'posix'), cwd: '/elsewhere' } })).isError, true);
  }

});

test('SDK abort during dense work suppresses the server response and permits subsequent tools', async t => {
  const { client, transport, root } = await fixture(t, 'allow', { dense: true });
  const sent = [], received = [];
  const send = transport.send.bind(transport);
  transport.send = async message => { sent.push(message); return send(message); };
  const onmessage = transport.onmessage;
  transport.onmessage = message => { received.push(message); onmessage?.(message); };
  const controller = new AbortController();
  const pending = client.callTool({ name: 'policy_evaluate', arguments: normalized(root, 'heavy', []) }, undefined, { signal: controller.signal, timeout: 5000 }).then(
    () => ({ succeeded: true }), error => ({ error }));
  // Ping is a dispatch barrier: the preceding tool is already admitted and its
  // evaluator worker started, while the input dispatcher can still answer ping.
  await client.ping();
  controller.abort();
  const outcome = await pending;
  assert.ok(outcome.error);
  const cancelled = sent.find(message => message.method === 'notifications/cancelled');
  assert.ok(cancelled);
  const next = await client.callTool({ name: 'policy_evaluate', arguments: normalized(root) });
  assert.equal(data(next).decision, 'allow');
  assert.ok(!received.some(message => message.id === cancelled.params.requestId),
    'a cancellation processed during evaluation must suppress its response');
});

test('SDK concurrent requests preserve IDs and separate connection candidates', async t => {
  const first = await fixture(t, 'allow');
  const second = await fixture(t, 'deny');
  const calls = await Promise.all(Array.from({ length: 8 }, (_, index) => {
    const client = index % 2 ? first.client : second.client;
    return client.callTool({ name: 'policy_evaluate', arguments: normalized(index % 2 ? first.root : second.root) });
  }));
  assert.deepEqual(calls.map(report => data(report).decision), ['deny', 'allow', 'deny', 'allow', 'deny', 'allow', 'deny', 'allow']);
  assert.equal((await first.client.callTool({ name: 'policy_evaluate', arguments: { ...normalized(first.root), project: second.root, approved: true } })).isError, true);
  const inspect = await first.client.callTool({ name: 'status' });
  assert.ok(data(inspect).active_candidate_id);

});

test('SDK reports core failures and pre-cancellation without changing policy', async t => {
  const { client, root } = await fixture(t);
  const invalid = await client.callTool({ name: 'policy_evaluate', arguments: normalized(root, '', []) });
  assert.equal(invalid.isError, true);
  assert.equal(invalid.structuredContent.error.code, 'invalid_input');
  const controller = new AbortController(); controller.abort();
  await assert.rejects(client.callTool({ name: 'policy_evaluate', arguments: normalized(root) }, undefined,
    { signal: controller.signal, timeout: 1000 }));
  const next = await client.callTool({ name: 'policy_evaluate', arguments: normalized(root) });
  assert.equal(data(next).decision, 'allow');
});
