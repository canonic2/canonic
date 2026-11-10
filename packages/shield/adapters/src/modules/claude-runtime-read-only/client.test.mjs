import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile, symlink, copyFile, rename } from 'node:fs/promises';
import { join } from 'node:path';
import { createHash, randomUUID } from 'node:crypto';
import { execFile } from 'node:child_process';
import { promisify } from 'node:util';
import { createClaudeRuntimeReadOnlyClient } from './index.mjs';
import { response } from '../runtime-response/index.mjs';
import { transport } from '../process-transport/index.mjs';
import { binary, enrolledRuntime, hostInput } from '../../../fixtures/runtime.mjs';

const exec = promisify(execFile);
const pre = client => client.hooks.PreToolUse[0].hooks[0];
const post = client => client.hooks.PostToolUse[0].hooks[0];
const decision = value => value.hookSpecificOutput.permissionDecision;
const TOKEN = 'SHIELD_SDK_READ_CONTEXT_TOKEN';
const supported = ['darwin', 'linux'].includes(process.platform);

const binaryDigest = createHash('sha256').update(await readFile(binary)).digest('hex');

async function mapping() {
  const hash = createHash('sha256').update('shield-sdk-read-mapping-v1\0');
  for (const path of ['claude-runtime-read-only/index.mjs', 'claude-runtime-read-only/binary.mjs', 'runtime-response/index.mjs',
    'enforcement-response/json.mjs', 'process-transport/index.mjs']) {
    for (const bytes of [Buffer.from(path), await readFile(new URL(`../${path}`, import.meta.url))]) {
      const length = Buffer.alloc(8);
      length.writeBigUInt64BE(BigInt(bytes.length));
      hash.update(length).update(bytes);
    }
  }
  return hash.digest('hex');
}

async function ownedContext(t, fixture) {
  await fixture.operator(['sdk', 'setup', '--adapter', 'claude-read-only']);
  const minted = await exec(binary, ['__internal', 'sdk-read-open', '--project', fixture.root, '--mapping-digest', await mapping()]);
  assert.match(minted.stdout, /^[a-f0-9]{64}\n$/);
  const env = { ...process.env, [TOKEN]: minted.stdout.trim() };
  let closed = false;
  const close = async () => {
    if (closed) return;
    await exec(binary, ['__internal', 'sdk-read-close', '--project', fixture.root], { env });
    closed = true;
  };
  t.after(() => close().catch(() => {}));
  let projectId;
  const invoke = async (operation, payload) => {
    const envelope = { protocol_version: 1, correlation_id: randomUUID(), operation, payload };
    const input = Buffer.from(JSON.stringify(envelope));
    const value = await transport({ binary, args: ['__internal', 'runtime-request', '--project', fixture.root, '--timeout-ms', '60000'],
      env, input, timeoutMs: 60000, expected: { correlation_id: envelope.correlation_id, operation, project_id: projectId,
        inputDigest: createHash('sha256').update(input).digest('hex'), request_id: payload.request_id,
        retry_ref: payload.retry_ref, reported_request_id: payload.request_id } }, response);
    if (value.error === null) projectId ??= value.project_id;
    return value;
  };
  invoke.close = close;
  return invoke;
}

test('general runtime evaluate inspects an approved private retry without consuming and explicit consume is once only', { skip: !supported }, async t => {
  const fixture = await enrolledRuntime(t);
  const invoke = await ownedContext(t, fixture);
  const input = { kind: 'file', action: 'read', sources: [fixture.asked], destination: null, recursive: false,
    original_input: { file_path: fixture.asked } };
  const context = { cwd: fixture.root, environment: null };
  const provenance = { session_id: 'acme-session', tool_use_id: 'tool-runtime-wire', event: 'PreToolUse' };
  const fresh = await invoke('runtime.evaluate', { input, context, provenance, retry_ref: null });
  assert.equal(fresh.error, null);
  assert.equal(fresh.result.decision, 'ask');
  assert.equal(fresh.result.consumed, false);
  const id = fresh.result.approval_request_id;
  const retry = fresh.result.retry_ref;
  assert.ok(id && retry);
  await fixture.operator(['request', 'approve', id]);
  const inspected = await invoke('runtime.evaluate', { input, context, provenance, retry_ref: retry });
  assert.equal(inspected.error, null);
  assert.equal(inspected.result.disposition, 'approval_required');
  assert.equal(inspected.result.consumed, false);
  assert.equal((await fixture.cli(['request', 'show', id])).status, 'approved');
  const consumed = await invoke('runtime.consume', { input, context, provenance, request_id: id, retry_ref: retry });
  assert.equal(consumed.error, null);
  assert.equal(consumed.result.consumed, true);
  assert.equal(consumed.result.disposition, 'allowed');
  const replay = await invoke('runtime.consume', { input, context, provenance, request_id: id, retry_ref: retry });
  assert.ok(replay.error !== null || replay.result.disposition === 'blocked');
  assert.equal((await fixture.events()).filter(event => event.kind === 'read_grant_consumed').length, 1);
  await invoke.close();
});

test('general runtime client approval stays in the original callback and completion retries retain evidence', { skip: !supported }, async t => {
  const fixture = await enrolledRuntime(t);
  await fixture.operator(['sdk', 'setup', '--adapter', 'claude-read-only']);
  let publicRequest;
  const client = await createClaudeRuntimeReadOnlyClient({ binary, binaryDigest, project: fixture.root, timeoutMs: 60000,
    onApprovalRequired: async (request, context) => {
      assert.deepEqual(Object.keys(request), ['request_id']);
      assert.ok(context.signal instanceof AbortSignal);
      publicRequest = request.request_id;
      await fixture.operator(['request', 'approve', request.request_id]);
    } });
  t.after(() => client.close().catch(() => {}));
  const input = hostInput(fixture.root, 'Read', { file_path: fixture.asked, offset: 1, limit: 1 }, 'tool-runtime-owned');
  const result = await pre(client)(input);
  assert.equal(decision(result), 'allow');
  assert.deepEqual(result.hookSpecificOutput.updatedInput, input.tool_input);
  assert.equal((await fixture.events()).filter(event => event.kind === 'read_grant_consumed').length, 1);
  assert.equal((await fixture.cli(['request', 'show', publicRequest])).status, 'consumed');
  const completed = { ...input, hook_event_name: 'PostToolUse', tool_response: 'PRIVATE_OUTPUT_SENTINEL' };
  assert.deepEqual(await post(client)(completed), {});
  assert.deepEqual(await post(client)(completed), {});
  const events = await fixture.events();
  const completions = events.filter(event => event.kind === 'completion');
  assert.equal(completions.length, 1);
  assert.equal(completions[0].links.request_id, publicRequest);
  assert.ok(completions[0].links.decision_event_id);
  assert.doesNotMatch(JSON.stringify(events), /PRIVATE_OUTPUT_SENTINEL/);
  await client.close();
});

test('general runtime client fresh invocation cannot select an earlier approval and reused host IDs cannot join', { skip: !supported }, async t => {
  const fixture = await enrolledRuntime(t);
  await fixture.operator(['sdk', 'setup', '--adapter', 'claude-read-only']);
  const client = await createClaudeRuntimeReadOnlyClient({ binary, binaryDigest, project: fixture.root, timeoutMs: 60000 });
  t.after(() => client.close().catch(() => {}));
  const input = hostInput(fixture.root, 'Read', { file_path: fixture.asked }, 'tool-runtime-first');
  assert.equal(decision(await pre(client)(input)), 'deny');
  const id = (await fixture.cli(['request', 'list'])).requests[0].request_id;
  await fixture.operator(['request', 'approve', id]);
  assert.equal(decision(await pre(client)({ ...input, tool_use_id: 'tool-runtime-new' })), 'deny');
  assert.equal((await fixture.cli(['request', 'show', id])).status, 'approved');
  const allowed = hostInput(fixture.root, 'Read', { file_path: fixture.allowed }, 'tool-runtime-reused');
  assert.equal(decision(await pre(client)(allowed)), 'allow');
  const completed = { ...allowed, hook_event_name: 'PostToolUse' };
  await post(client)(completed);
  await post(client)(completed);
  assert.equal((await fixture.events()).filter(event => event.kind === 'completion').length, 1);
  assert.equal(decision(await pre(client)(allowed)), 'allow');
  await post(client)(completed);
  const completions = (await fixture.events()).filter(event => event.kind === 'completion');
  assert.equal(completions.at(-1).links.request_id, null);
  const alias = join(fixture.root, 'cwd-alias');
  await symlink(fixture.root, alias);
  const aliased = { ...allowed, cwd: alias, tool_use_id: 'tool-runtime-alias' };
  assert.equal(decision(await pre(client)(aliased)), 'allow');
  await post(client)({ ...aliased, hook_event_name: 'PostToolUse' });
  const aliasEvents = await fixture.events();
  const aliasDecision = aliasEvents.find(event => event.kind === 'decision' && event.provenance.tool_use_id === aliased.tool_use_id);
  assert.equal(aliasDecision.details.reported_cwd, alias);
  assert.equal(aliasDecision.provenance.cwd, fixture.root);
  assert.equal(aliasEvents.filter(event => event.kind === 'completion').at(-1).links.decision_event_id, aliasDecision.event_id);
  await client.close();
});

test('general runtime client caps eight owned callbacks without queueing a ninth or retaining children on close', { timeout: 120000, skip: !supported }, async t => {
  const fixture = await enrolledRuntime(t);
  await fixture.operator(['sdk', 'setup', '--adapter', 'claude-read-only']);
  let release;
  const wait = new Promise(resolve => { release = resolve; });
  let arrived;
  let count = 0;
  const client = await createClaudeRuntimeReadOnlyClient({ binary, binaryDigest, project: fixture.root, timeoutMs: 60000,
    onApprovalRequired: async () => { count++; arrived(); await wait; } });
  const calls = [];
  try {
    for (let index = 0; index < 8; index++) {
      const ready = new Promise(resolve => { arrived = resolve; });
      calls.push(pre(client)(hostInput(fixture.root, 'Read', { file_path: fixture.asked }, `tool-bounded-${index}`)));
      await Promise.race([ready, calls.at(-1).then(() => { throw Error('Owned callback failed before its approval wait'); })]);
    }
    const before = (await fixture.cli(['request', 'list'])).requests.map(row => row.request_id).sort();
    const ninth = await pre(client)(hostInput(fixture.root, 'Read', { file_path: fixture.asked }, 'tool-bounded-overload'));
    assert.equal(decision(ninth), 'deny');
    assert.equal(count, 8);
    assert.deepEqual((await fixture.cli(['request', 'list'])).requests.map(row => row.request_id).sort(), before);
    const completion = await post(client)({ ...hostInput(fixture.root, 'Read', { file_path: fixture.allowed }, 'tool-bounded-post'), hook_event_name: 'PostToolUse' });
    assert.match(completion.systemMessage, /history unavailable/);
    await client.close();
    assert.ok((await Promise.all(calls)).every(value => decision(value) === 'deny'));
  } finally { release(); await client.close().catch(() => {}); await Promise.allSettled(calls); }
});

test('general runtime new client reuses reviewed content identity after the selected binary moves', { skip: !supported }, async t => {
  const fixture = await enrolledRuntime(t);
  await fixture.operator(['sdk', 'setup', '--adapter', 'claude-read-only']);
  const original = join(fixture.root, 'shield-first');
  const moved = join(fixture.root, 'shield-moved');
  await copyFile(binary, original);
  for (const selected of [original, moved]) {
    if (selected === moved) await rename(original, moved);
    const client = await createClaudeRuntimeReadOnlyClient({ binary: selected, binaryDigest, project: fixture.root, timeoutMs: 60000 });
    try {
      assert.equal(decision(await pre(client)(hostInput(fixture.root, 'Read', { file_path: fixture.allowed }, `tool-${selected === moved ? 'moved' : 'first'}`))), 'allow');
    } finally { await client.close(); }
  }
});
