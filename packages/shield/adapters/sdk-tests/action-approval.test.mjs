import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { join } from 'node:path';
import { query } from '@anthropic-ai/claude-agent-sdk';
import { createClaudeReadOnlySession } from '../src/modules/claude-read-only/index.mjs';
import { binary, enrolledRuntime } from '../fixtures/runtime.mjs';
import { scriptedProvider } from './claude-provider.mjs';

for (const action of ['accept', 'decline']) {
  test(`interactive SDK action approval ${action} uses its own form and exact read consumer`, { timeout: 60000 }, async t => {
    const f = await enrolledRuntime(t);
    // Explicit initial human review enables this project's MCP form channel.
    await f.operator(['mcp', 'setup', '--agent', 'claude']);
    const before = JSON.parse(await readFile(f.statePath, 'utf8')).active;
    const forms = [];
    const options = await createClaudeReadOnlySession({ binary, project: f.root,
      onElicitation: async request => {
        forms.push(request);
        assert.equal(request.serverName, 'shield');
        assert.match(request.message, /SDK text read/);
        assert.ok(request.message.includes(f.asked));
        assert.doesNotMatch(request.message, /ACME_ASKED_TEXT/);
        const state = JSON.parse(await readFile(f.statePath, 'utf8'));
        assert.equal(state.read_requests[0].status, 'pending');
        assert.deepEqual(state.active, before);
        return { action, content: {} };
      } });
    const decisions = [];
    const pre = options.hooks.PreToolUse[0].hooks[0];
    const hooks = { ...options.hooks, PreToolUse: [{ timeout: options.hooks.PreToolUse[0].timeout,
      hooks: [async (...args) => { const result = await pre(...args); decisions.push([args[0].tool_name, result.hookSpecificOutput.permissionDecision]); return result; }] }] };
    const provider = await scriptedProvider(t, [
      { name: 'Read', input: { file_path: f.asked } },
      { name: 'mcp__shield__request_approve', input: async () => {
        const state = JSON.parse(await readFile(f.statePath, 'utf8'));
        assert.equal(state.read_requests[0].origin.agent, 'claude-sdk');
        assert.equal(state.read_requests[0].origin.session_id, options.sessionId);
        return { request_id: (await f.cli(['request', 'list'])).requests[0].request_id };
      } },
      { name: 'Read', input: { file_path: f.asked } },
      ...(action === 'accept' ? [{ name: 'Read', input: { file_path: f.asked } }] : []),
    ]);
    const conversation = query({ prompt: 'Perform the Acme fixture tools in order.', options: { ...options, hooks,
      cwd: f.root, persistSession: false, maxTurns: 10,
      env: { ...process.env, HOME: f.root, ANTHROPIC_API_KEY: 'acme-local-test-key', ANTHROPIC_BASE_URL: provider.url,
        CLAUDE_CONFIG_DIR: join(f.root, 'claude-config'), CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC: '1' } } });
    t.after(() => conversation.close());
    const messages = [];
    for await (const message of conversation) messages.push(message);
    assert.ok(messages.some(message => message.type === 'result' && message.subtype === 'success'), JSON.stringify(messages));
    assert.equal(forms.length, 1, JSON.stringify(provider.results));
    assert.deepEqual(decisions.filter(([name]) => name === 'Read').map(([, decision]) => decision), action === 'accept' ? ['deny', 'allow', 'deny'] : ['deny', 'deny']);
    assert.equal(decisions.find(([name]) => name === 'mcp__shield__request_approve')?.[1], 'allow');
    const state = JSON.parse(await readFile(f.statePath, 'utf8'));
    assert.deepEqual(state.active, before);
    // Consumed read grants are reclaimed from minimum authority immediately;
    // their optional history remains separate from consumption authority.
    const issued = (await f.events()).filter(event => event.kind === 'read_grant_issued');
    assert.equal(issued.length, action === 'accept' ? 1 : 0);
    if (action === 'accept') assert.equal(issued[0].operator.mechanism, 'host-elicitation');
    assert.equal(new Set(provider.results.filter(result => JSON.stringify(result).includes('ACME_ASKED_TEXT')).map(result => result.tool_use_id)).size, action === 'accept' ? 1 : 0);
  });
}
