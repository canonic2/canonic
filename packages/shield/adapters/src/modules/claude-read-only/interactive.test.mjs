import test from 'node:test';
import assert from 'node:assert/strict';
import { createClaudeReadOnlySession } from './index.mjs';
import { binary, enrolledRuntime, hostInput } from '../../../fixtures/runtime.mjs';

test('interactive SDK factory scopes approval tools to its captured reads and explicit session', async t => {
  const f = await enrolledRuntime(t);
  let forms = 0;
  const options = await createClaudeReadOnlySession({ binary, project: f.root, onElicitation: async () => { forms++; return { action: 'decline' }; } });
  const pre = options.hooks.PreToolUse[0].hooks[0];
  const input = (name, args, session = options.sessionId) => ({ ...hostInput(f.root, name, args), session_id: session });
  const read = await pre(input('Read', { file_path: f.asked }));
  assert.equal(read.hookSpecificOutput.permissionDecision, 'deny');
  assert.match(read.hookSpecificOutput.permissionDecisionReason, /mcp__shield__request_approve/);
  const id = (await f.cli(['request', 'list'])).requests[0].request_id;
  assert.equal((await pre(input('mcp__shield__request_approve', { request_id: id }))).hookSpecificOutput.permissionDecision, 'allow');
  for (const [name, args, session] of [
    ['mcp__shield__request_approve', { request_id: 'read-request-foreign' }],
    ['mcp__other__request_approve', { request_id: id }],
    ['mcp__shield__request_approve', { request_ids: [id, id] }],
    ['mcp__shield__request_approve', { request_id: id, confirmed: true }],
    ['mcp__shield__request_approve', { request_id: id }, 'other-session'],
    ['mcp__shield__policy_apply', {}], ['Bash', { command: 'echo Acme' }],
  ]) assert.equal((await pre(input(name, args, session))).hookSpecificOutput.permissionDecision, 'deny', name);
  assert.deepEqual(await options.onElicitation({ serverName: 'other', mode: 'form' }), { action: 'decline' });
  assert.deepEqual(await options.onElicitation({ serverName: 'shield', mode: 'url' }), { action: 'decline' });
  assert.equal(forms, 0);
  await options.onElicitation({ serverName: 'shield' });
  assert.equal(forms, 1);
  await assert.rejects(createClaudeReadOnlySession({ binary, project: f.root }));
  await assert.rejects(createClaudeReadOnlySession({ binary: 'relative', project: f.root, onElicitation: async () => ({ action: 'accept' }) }));
});
