import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile, writeFile, symlink, link, rm, chmod, mkdir } from 'node:fs/promises';
import { join } from 'node:path';
// Capture real polling before virtualizing the transport's deadline timers.
import { setTimeout as wait } from 'node:timers/promises';
import { createClaudeReadOnlyHooks } from './index.mjs';
import { binary, enrolledRuntime, hostInput, authorizationState } from '../../../fixtures/runtime.mjs';

const callback = hooks => hooks.PreToolUse[0].hooks[0];
const decision = result => result.hookSpecificOutput.permissionDecision;

test('missing compiled policy recovers and unrecoverable cache still blocks', async t => {
  const fixture = await enrolledRuntime(t);
  const call = callback(await createClaudeReadOnlyHooks({ binary, project: fixture.root }));
  const { active: { compiled } } = JSON.parse(await readFile(fixture.statePath, 'utf8'));
  const path = join(compiled.directory, 'compiled', compiled.project_key, `${compiled.plan_key}.bin`);
  await rm(path);
  const input = hostInput(fixture.root, 'Read', { file_path: fixture.allowed });
  assert.equal(decision(await call(input)), 'allow');
  assert.deepEqual(authorizationState(await readFile(fixture.statePath)), authorizationState(fixture.state));
  await readFile(path);
  await rm(path);
  await mkdir(path);
  const blocked = await call(input);
  assert.equal(decision(blocked), 'deny');
  assert.ok(blocked.hookSpecificOutput.permissionDecisionReason.includes(`shield cache prepare --project '${fixture.root}'`));
  assert.deepEqual(authorizationState(await readFile(fixture.statePath)), authorizationState(fixture.state));
  await rm(path, { recursive: true });
  assert.equal(decision(await call(input)), 'allow');
});

test('operator-approved read is consumed once across concurrent SDK retries', async t => {
  const fixture = await enrolledRuntime(t);
  const call = callback(await createClaudeReadOnlyHooks({ binary, project: fixture.root }));
  const input = hostInput(fixture.root, 'Read', { file_path: fixture.asked, offset: 1, limit: 1 });
  assert.equal(decision(await call(input)), 'deny');
  const rows = (await fixture.cli(['request', 'list'])).requests;
  assert.equal(rows.length, 1);
  const id = rows[0].request_id;
  const stored = await fixture.cli(['request', 'show', id]);
  assert.deepEqual(stored.request.arguments, [fixture.asked, '--offset', '1', '--limit', '1']);
  await fixture.operator(['request', 'approve', id]);
  assert.equal(decision(await call({ ...input, tool_use_id: 'tool-acme-single' })), 'allow');
  const observed = await fixture.events();
  assert.equal(observed.filter(e => e.kind === 'read_grant_consumed').length, 1);
  assert.ok(observed.some(e => e.kind === 'decision' && e.details.host_decision === 'allow' && e.details.decision === 'ask' && e.links.approval_id));
  // History is best effort under contention; check its receipt before racing consumers.
  assert.equal(decision(await call({ ...input, tool_use_id: 'tool-acme-pending' })), 'deny');
  const next = (await fixture.cli(['request', 'list'])).requests[0].request_id;
  await fixture.operator(['request', 'approve', next]);
  const retries = await Promise.all([2, 3, 4].map(n => call({ ...input, tool_use_id: `tool-acme-${n}` })));
  assert.equal(retries.filter(value => decision(value) === 'allow').length, 1);
  await assert.rejects(fixture.cli(['request', 'show', id]));
  assert.equal(decision(await call({ ...input, tool_use_id: 'tool-acme-5' })), 'deny');
  const events = (await fixture.events());
  assert.equal(events.filter(e => e.kind === 'read_grant_consumed').length, 2);
});

test('approval does not cross factory session window or changed file bindings', async t => {
  const fixture = await enrolledRuntime(t);
  const call = callback(await createClaudeReadOnlyHooks({ binary, project: fixture.root }));
  const input = hostInput(fixture.root, 'Read', { file_path: fixture.asked });
  await call(input);
  const id = (await fixture.cli(['request', 'list'])).requests[0].request_id;
  await fixture.operator(['request', 'approve', id]);
  const other = callback(await createClaudeReadOnlyHooks({ binary, project: fixture.root }));
  assert.equal(decision(await other(input)), 'deny');
  assert.equal(decision(await call({ ...input, session_id: 'other-session' })), 'deny');
  assert.equal(decision(await call({ ...input, tool_input: { file_path: fixture.asked, limit: 1 } })), 'deny');
  assert.equal(decision(await call({ ...input, approved: true })), 'deny');
  await writeFile(fixture.asked, 'CHANGED_ACME_TEXT');
  assert.equal(decision(await call(input)), 'deny');
  assert.equal((await fixture.cli(['request', 'show', id])).status, 'approved');
  await fixture.operator(['request', 'deny', id]);
  await assert.rejects(fixture.operator(['request', 'approve', id]));
});

test('registered read callback preserves captured window and reloads active policy', async t => {
  const fixture = await enrolledRuntime(t);
  const options = { binary, project: fixture.root };
  const preparing = createClaudeReadOnlyHooks(options);
  options.binary = '/missing';
  const hooks = await preparing;
  assert.equal(hooks.PreToolUse[0].matcher, undefined);
  const call = callback(hooks);
  const input = hostInput(fixture.root, 'Read', { file_path: fixture.allowed, offset: 1, limit: 2 });
  const pending = call(input, input.tool_use_id);
  input.tool_input.file_path = fixture.denied;
  input.tool_input.limit = 999;
  const result = await pending;
  assert.equal(decision(result), 'allow');
  assert.deepEqual(result.hookSpecificOutput.updatedInput, { file_path: fixture.allowed, offset: 1, limit: 2 });
  for (const path of [fixture.denied, fixture.asked]) {
    assert.equal(decision(await call(hostInput(fixture.root, 'Read', { file_path: path }))), 'deny');
  }
  assert.deepEqual(authorizationState(await readFile(fixture.statePath)), authorizationState(fixture.state));
  const path = join(fixture.root, '.shield/rules.yml');
  const rules = await fixture.readRules();
  rules.rules = rules.rules.filter(rule => rule.id !== 'read');
  await writeFile(path, JSON.stringify(rules));
  await fixture.operator(['policy', 'review'], 'manual\nyes\n');
  assert.equal(decision(await call(hostInput(fixture.root, 'Read', { file_path: fixture.allowed }))), 'deny');
});

test('all shell, write and unknown tools block despite literal policy allowances', async t => {
  const fixture = await enrolledRuntime(t);
  const call = callback(await createClaudeReadOnlyHooks({ binary, project: fixture.root }));
  for (const [tool, input] of [['Bash', { command: `${binary} init --project ${fixture.root}` }],
    ['Write', { file_path: fixture.statePath, content: 'tampered' }], ['Edit', {}],
    ['mcp__acme__approve', { approved: true }], ['Agent', {}], ['PowerShell', {}]]) {
    const result = await call(hostInput(fixture.root, tool, input));
    assert.equal(decision(result), 'deny');
    assert.match(result.hookSpecificOutput.permissionDecisionReason, /unsupported-operation/);
  }
  assert.deepEqual(authorizationState(await readFile(fixture.statePath)), authorizationState(fixture.state));
});

test('read binding rejects state, symlinks, hardlinks, nontext and oversized files', async t => {
  const fixture = await enrolledRuntime(t);
  const call = callback(await createClaudeReadOnlyHooks({ binary, project: fixture.root }));
  const linked = join(fixture.root, 'symbolic.txt');
  const hard = join(fixture.root, 'hard.txt');
  const nontext = join(fixture.root, 'nontext.txt');
  const oversized = join(fixture.root, 'oversized.txt');
  const pdf = join(fixture.root, 'pdf.txt');
  const reserved = join(fixture.root, '.shield/reserved.txt');
  await symlink(fixture.allowed, linked);
  await link(fixture.allowed, hard);
  await writeFile(nontext, Buffer.from([0, 255]));
  await writeFile(oversized, 'x'.repeat(65_537));
  await writeFile(pdf, '%PDF-1.4\n');
  await writeFile(reserved, 'private retained bytes');
  for (const path of [linked, hard, nontext, oversized, pdf, reserved, fixture.statePath]) {
    assert.equal(decision(await call(hostInput(fixture.root, 'Read', { file_path: path }))), 'deny');
  }
  await rm(hard);
  assert.equal(decision(await call(hostInput(fixture.root, 'Read', { file_path: fixture.allowed }))), 'allow');
});

test('invalid host claims, mismatched IDs, changed enrollment and cancelled calls block', async t => {
  const fixture = await enrolledRuntime(t);
  const call = callback(await createClaudeReadOnlyHooks({ binary, project: fixture.root }));
  const input = hostInput(fixture.root, 'Read', { file_path: fixture.allowed });
  for (const value of [{ ...input, approved: true }, { ...input, cwd: '/' },
    { ...input, tool_input: { file_path: fixture.allowed, pages: '1' } },
    { ...input, tool_input: { file_path: fixture.allowed, limit: 0 } }]) {
    assert.equal(decision(await call(value)), 'deny');
  }
  assert.equal(decision(await call(input, 'another-tool')), 'deny');
  assert.equal(decision(await call(input, input.tool_use_id, { signal: AbortSignal.abort() })), 'deny');
  await rm(join(fixture.root, '.shield'), { recursive: true });
  await fixture.initialize();
  assert.equal(decision(await call(input)), 'deny');
});

async function evaluator(root, variant, registrationDelayMs = 0) {
  const path = join(root, `evaluator-${variant}.mjs`);
  await writeFile(path, `#!/usr/bin/env node
import { readFileSync, writeFileSync, renameSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
const bytes = readFileSync(0);
const input = JSON.parse(bytes);
const variant = ${JSON.stringify(variant)};
const publish = (path, value) => { writeFileSync(path + '.tmp', value); renameSync(path + '.tmp', path); };
if (variant === 'hang' && input.operation.kind !== 'register') {
  process.on('SIGTERM', () => publish(${JSON.stringify(join(root, 'evaluator.term'))}, 'TERM'));
  publish(${JSON.stringify(join(root, 'evaluator.pid'))}, String(process.pid));
  setInterval(() => {}, 1000);
} else {
const result = spawnSync(${JSON.stringify(binary)}, process.argv.slice(2), { input: bytes });
if (input.operation.kind === 'register') {
  await new Promise(resolve => setTimeout(resolve, ${registrationDelayMs}));
  process.stdout.write(result.stdout); process.exit(result.status);
}
const value = JSON.parse(result.stdout);
if (variant === 'path') value.operation.path += '.other';
if (variant === 'id') value.request_id = 'another-tool';
if (variant === 'project') value.identity.project_id = 'another-project';
if (variant === 'revision') value.identity.policy_revision = '';
if (variant === 'missing-diagnostics') delete value.diagnostics;
if (variant === 'history') value.history = { status: 'failed', request_id: null, event_id: null };
if (variant === 'grant') { value.code = 'allowed-once'; value.approval = { status: 'consumed', request_id: 'fabricated', approval_id: 'fabricated' }; }
if (variant === 'lost-grant' && value.code === 'allowed-once') process.exit(1);
if (variant === 'crash') process.exit(1);
if (variant === 'empty') process.exit(0);
if (variant === 'invalid-utf8') value.diagnostics.message = 'ACME_MARKER';
const output = JSON.stringify(value);
if (variant === 'duplicate-key') {
  process.stdout.write(output.replace('"decision":', '"decision":"deny","decision":')); process.exit(0);
}
if (variant === 'invalid-utf8') {
  const [before, after] = output.split('ACME_MARKER');
  process.stdout.write(Buffer.concat([Buffer.from(before), Buffer.from([0xff]), Buffer.from(after)])); process.exit(0);
}
process.stdout.write(output); process.exit(variant === 'exit' ? 2 : 0);
}
`);
  await chmod(path, 0o700);
  return path;
}

test('malformed, mismatched, failed and unavailable evaluators never return host allow', { timeout: 120000 }, async t => {
  const fixture = await enrolledRuntime(t);
  // Receipt validation is the subject here; dispatch deadlines are checked below.
  // Keep real Rust/process I/O, but do not let host scheduling choose the injected fault.
  t.mock.timers.enable({ apis: ['setTimeout'] });
  const input = hostInput(fixture.root, 'Read', { file_path: fixture.allowed });
  for (const variant of ['path', 'id', 'project', 'revision', 'missing-diagnostics', 'grant', 'crash', 'empty', 'exit',
    'duplicate-key', 'invalid-utf8']) {
    const selected = await evaluator(fixture.root, variant);
    const call = callback(await createClaudeReadOnlyHooks({ binary: selected, project: fixture.root }));
    const result = await call(input);
    assert.equal(decision(result), 'deny', variant);
    assert.match(result.hookSpecificOutput.permissionDecisionReason, /protection-failure/);
  }
  await assert.rejects(createClaudeReadOnlyHooks({ binary: '/missing-shield', project: fixture.root }));
  const call = callback(await createClaudeReadOnlyHooks({ binary, project: fixture.root }));
  await writeFile(fixture.statePath, 'corrupt');
  assert.equal(decision(await call(input)), 'deny');
});

test('a lost allowed-once reply leaves the durable grant consumed', { timeout: 120000 }, async t => {
  const fixture = await enrolledRuntime(t);
  // Reach the real durable consumption before injecting the lost reply.
  t.mock.timers.enable({ apis: ['setTimeout'] });
  const selected = await evaluator(fixture.root, 'lost-grant');
  const call = callback(await createClaudeReadOnlyHooks({ binary: selected, project: fixture.root }));
  const input = hostInput(fixture.root, 'Read', { file_path: fixture.asked });
  assert.equal(decision(await call(input)), 'deny');
  const id = (await fixture.cli(['request', 'list'])).requests[0].request_id;
  await fixture.operator(['request', 'approve', id]);
  const lost = await call({ ...input, tool_use_id: 'tool-acme-retry' });
  assert.equal(decision(lost), 'deny');
  assert.match(lost.hookSpecificOutput.permissionDecisionReason, /protection-failure/);
  await assert.rejects(fixture.cli(['request', 'show', id]));
  assert.equal((await fixture.events()).filter(e => e.kind === 'read_grant_consumed').length, 1);
  assert.match((await call({ ...input, tool_use_id: 'tool-acme-again' })).hookSpecificOutput.permissionDecisionReason, /approval-unavailable/);
  await assert.rejects(fixture.operator(['request', 'approve', id]));
});

test('optional history failure is visible and cannot change a protection decision', async t => {
  const fixture = await enrolledRuntime(t);
  const selected = await evaluator(fixture.root, 'history');
  const hooks = await createClaudeReadOnlyHooks({ binary: selected, project: fixture.root });
  const pre = callback(hooks);
  const input = hostInput(fixture.root, 'Read', { file_path: fixture.allowed });
  const allowed = await pre(input);
  assert.equal(decision(allowed), 'allow');
  assert.match(allowed.systemMessage, /history unavailable/);
  const blocked = await pre(hostInput(fixture.root, 'Read', { file_path: fixture.denied }));
  assert.equal(decision(blocked), 'deny');
  assert.match(blocked.systemMessage, /history unavailable/);
  const result = await hooks.PostToolUse[0].hooks[0]({ ...input, hook_event_name: 'PostToolUse', tool_response: 'PRIVATE_OUTPUT' });
  assert.match(result.systemMessage, /history unavailable/);
  assert.equal(result.hookSpecificOutput, undefined);
});

test('deadline and cancellation deny and reap the owned evaluator that ignores SIGTERM', async t => {
  const fixture = await enrolledRuntime(t);
  // Deliberately make real registration slower than the dispatch deadline. Fixture
  // readiness is not a latency assertion; the deadline and grace period are exact.
  const selected = await evaluator(fixture.root, 'hang', 1500);
  t.mock.timers.enable({ apis: ['setTimeout'] });
  const call = callback(await createClaudeReadOnlyHooks({ binary: selected, project: fixture.root, timeoutMs: 1000 }));
  const input = hostInput(fixture.root, 'Read', { file_path: fixture.allowed });
  for (const cancel of [false, true]) {
    const controller = new AbortController();
    const pending = call(input, input.tool_use_id, { signal: controller.signal });
    try {
      const pid = Number(await marker(join(fixture.root, 'evaluator.pid')));
      assert.ok(Number.isSafeInteger(pid) && pid > 0, 'the marker identifies one owned child');
      let settled = false;
      pending.then(() => { settled = true; });
      if (cancel) controller.abort();
      else {
        t.mock.timers.tick(999);
        await wait(10);
        assert.equal(settled, false, 'dispatch must remain pending before its deadline');
        assert.doesNotThrow(() => process.kill(pid, 0));
        t.mock.timers.tick(1);
      }
      assert.equal(await marker(join(fixture.root, 'evaluator.term')), 'TERM');
      t.mock.timers.tick(249);
      await wait(10);
      assert.equal(settled, false, 'ignored TERM must retain the full grace period');
      assert.doesNotThrow(() => process.kill(pid, 0));
      t.mock.timers.tick(1);
      assert.equal(decision(await pending), 'deny');
      assert.throws(() => process.kill(pid, 0), error => error.code === 'ESRCH');
    } finally {
      // A failed assertion must still release and reap this test-owned child.
      controller.abort();
      t.mock.timers.tick(250);
      await pending;
    }
    await rm(join(fixture.root, 'evaluator.pid'));
    await rm(join(fixture.root, 'evaluator.term'));
  }
});

async function marker(path) {
  const deadline = Date.now() + 10_000;
  while (Date.now() < deadline) {
    try { return await readFile(path, 'utf8'); }
    catch (error) { if (error.code !== 'ENOENT') throw error; }
    await wait(10);
  }
  assert.fail(`owned evaluator did not publish ${path}`);
}

test('history correlates exact reads, deduplicates completion and respects missing evidence, opt-out and purge', async t => {
  const fixture = await enrolledRuntime(t);
  const hooks = await createClaudeReadOnlyHooks({ binary, project: fixture.root });
  const pre = callback(hooks);
  const post = hooks.PostToolUse[0].hooks[0];
  const input = hostInput(fixture.root, 'Read', { file_path: fixture.allowed, offset: 1 }, 'tool-history');
  assert.equal(decision(await pre(input)), 'allow');
  const completed = { ...input, hook_event_name: 'PostToolUse', tool_response: { content: 'SECRET_OUTPUT_SENTINEL' } };
  assert.deepEqual(await post(completed), {});
  assert.deepEqual(await post(completed), {});
  let events = (await fixture.events());
  let decisions = events.filter(e => e.kind === 'decision');
  let completions = events.filter(e => e.kind === 'completion');
  assert.equal(completions.length, 1);
  assert.equal(completions[0].links.decision_event_id, decisions[0].event_id);
  assert.equal(completions[0].links.request_id, decisions[0].links.request_id);
  assert.equal(completions[0].details.status, 'completed');
  assert.equal(completions[0].details.exit_code, null);
  assert.doesNotMatch(JSON.stringify(events), /SECRET_OUTPUT_SENTINEL/);
  assert.deepEqual(await post({ ...completed, tool_input: { file_path: fixture.allowed, offset: 2 } }), {});
  assert.deepEqual(await post({ ...completed, session_id: 'other-session' }), {});
  events = (await fixture.events());
  completions = events.filter(e => e.kind === 'completion');
  assert.equal(completions.length, 3);
  assert.equal(completions[1].links.decision_event_id, null);
  assert.equal(completions[2].links.request_id, null);
  assert.equal(decision(await pre(input)), 'allow');
  await post(completed);
  completions = (await fixture.events()).filter(e => e.kind === 'completion');
  assert.equal(completions.at(-1).links.request_id, null, 'reused host IDs cannot identify a completion');
  assert.equal(decision(await pre(hostInput(fixture.root, 'Read', { file_path: fixture.asked }, 'tool-ask'))), 'deny');
  decisions = (await fixture.events()).filter(e => e.kind === 'decision');
  assert.equal(decisions.at(-1).details.decision, 'ask');
  assert.equal(decisions.at(-1).details.host_decision, 'deny');
  assert.equal(decisions.at(-1).details.execution, 'unobserved');
  await fixture.historySettings(false);
  const count = (await fixture.events()).length;
  assert.equal(decision(await pre(hostInput(fixture.root, 'Read', { file_path: fixture.allowed }, 'tool-opt-out'))), 'allow');
  await post({ ...completed, tool_use_id: 'tool-opt-out', tool_input: { file_path: fixture.allowed } });
  assert.equal((await fixture.events()).length, count);
  await fixture.historySettings(false, true);
  assert.deepEqual((await fixture.events()), []);
  assert.deepEqual(authorizationState(await readFile(fixture.statePath)), authorizationState(fixture.state));
});

test('post-tool history retains the original activation after operator reactivation', async t => {
  const fixture = await enrolledRuntime(t);
  const hooks = await createClaudeReadOnlyHooks({ binary, project: fixture.root });
  const input = hostInput(fixture.root, 'Read', { file_path: fixture.allowed }, 'tool-original-activation');
  assert.equal(decision(await callback(hooks)(input)), 'allow');
  const active = await fixture.reactivate();
  assert.notEqual(active.activation_id, fixture.active.activation_id);
  assert.deepEqual(await hooks.PostToolUseFailure[0].hooks[0]({ ...input, hook_event_name: 'PostToolUseFailure',
    error: 'PRIVATE_ERROR_SENTINEL', is_interrupt: true }), {});
  const events = (await fixture.events());
  const before = events.find(e => e.kind === 'decision');
  const after = events.find(e => e.kind === 'completion');
  assert.equal(before.details.activation_id, fixture.active.activation_id);
  assert.equal(after.links.decision_event_id, before.event_id);
  assert.equal(after.details.status, 'cancelled');
  assert.equal(after.details.exit_code, null);
  assert.doesNotMatch(JSON.stringify(events), /PRIVATE_ERROR_SENTINEL/);
});
