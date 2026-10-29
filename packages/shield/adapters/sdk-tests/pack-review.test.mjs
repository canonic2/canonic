// The real Claude host reviews a large pack through MCP and asks only at apply.
import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile, writeFile, mkdir } from 'node:fs/promises';
import { join } from 'node:path';
import { createHash } from 'node:crypto';
import { execFile } from 'node:child_process';
import { promisify } from 'node:util';
import { query } from '@anthropic-ai/claude-agent-sdk';
import { enrolledRuntime } from '../fixtures/runtime.mjs';
import { scriptedProvider } from './claude-provider.mjs';

const exec = promisify(execFile);

function returnedData(body) {
  const results = body.messages.flatMap(message => Array.isArray(message.content) ? message.content : [])
    .filter(block => block.type === 'tool_result');
  return results.flatMap(result => Array.isArray(result.content) ? result.content : [{ text: result.content }])
    .flatMap(block => {
      try { return [JSON.parse(block.text).data]; } catch { return []; }
    }).filter(Boolean);
}

for (const behavior of ['allow', 'deny']) {
  test(`Claude pack review needs only the final policy approval and honors ${behavior}`, { timeout: 60000 }, async t => {
    const f = await enrolledRuntime(t);
    const source = join(f.root, 'registry');
    await mkdir(source);
    const pack = JSON.stringify({ schema_version: 1, kind: 'pack', id: 'acme/runtime', version: '1.0.0',
      description: 'Acme runtime inspection; project execution remains outside these rules.',
      license: 'MIT', maintainers: ['Acme'], engines: { shield: '*' },
      capabilities: ['structured-rules-v1', 'argument-rules-v1'],
      rules: Array.from({ length: 192 }, (_, index) => ({ id: `inspect-${index}`, reason: `Inspect Acme runtime metadata group ${index % 36}.`,
        conditions: { domain: 'argv', executable: 'acme', arguments: { kind: 'exact', values: [`inspect-${index}`] } },
        decision: { loose: 'allow', recommended: 'allow', strict: 'ask' } })),
    });
    await writeFile(join(source, 'pack.json'), pack);
    await writeFile(join(source, 'catalog.json'), JSON.stringify({ schema_version: 1, packs: [{ id: 'acme/runtime',
      version: '1.0.0', path: 'pack.json', sha256: createHash('sha256').update(pack).digest('hex') }] }));
    await f.operator(['registry', 'add', 'acme', source]);
    await f.operator(['mcp', 'setup', '--agent', 'claude']);
    const before = JSON.parse(await readFile(f.statePath, 'utf8'));
    const reference = 'acme/acme/runtime';
    const provider = await scriptedProvider(t, [
      { name: 'mcp__shield__pack_info', input: { reference } },
      { name: 'mcp__shield__pack_info', input: { reference, rule_id: 'inspect-0' }, before: async () => {
        const summary = returnedData(provider.requests.at(-1)).find(data => data.view === 'summary');
        assert.equal(summary.rule_count, 192);
        assert.equal(summary.permission_group_count, 36);
        assert.equal(summary.truncated, false);
        assert.equal(summary.rules, undefined);
        assert.equal(summary.profiles.recommended.rule_outcomes.allow, 192);
      } },
      { name: 'mcp__shield__policy_propose', input: { change: { kind: 'pack_add', reference }, agent_reason: 'Review Acme runtime inspection permissions.' } },
      { name: 'mcp__shield__policy_apply', input: body => {
        const proposal = returnedData(body).find(data => data.apply_arguments);
        assert.ok(proposal?.apply_arguments, JSON.stringify(body.messages));
        return proposal.apply_arguments;
      } },
    ]);
    const prompts = [];
    const executable = (await exec('which', ['claude'])).stdout.trim();
    const conversation = query({ prompt: 'Review the Acme runtime pack and propose it for my approval.', options: {
      pathToClaudeCodeExecutable: executable, permissionMode: 'default',
      cwd: f.root, settingSources: ['project', 'local'], tools: ['Read', 'Bash'], persistSession: false, maxTurns: 8,
      canUseTool: async name => {
        assert.fail(`Unexpected tool permission prompt: ${name}`);
      },
      onElicitation: async request => {
        prompts.push(request.serverName);
        assert.match(request.message, /Add the runtime rule pack — version 1.0.0, recommended profile/);
        assert.match(request.message, /Without asking:/);
        assert.doesNotMatch(request.message, /candidate_digest|proposal_id|apply_arguments/);
        assert.deepEqual(request.requestedSchema.properties, {});
        assert.deepEqual(JSON.parse(await readFile(f.statePath, 'utf8')).active, before.active);
        return behavior === 'allow' ? { action: 'accept', content: {} } : { action: 'decline' };
      },
      env: { ...process.env, ANTHROPIC_API_KEY: 'acme-local-test-key', ANTHROPIC_BASE_URL: provider.url,
        CLAUDE_CONFIG_DIR: join(f.root, 'claude-config'), CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC: '1' },
    } });
    t.after(() => conversation.close());
    const messages = [];
    for await (const message of conversation) messages.push(message);
    assert.ok(messages.some(message => message.type === 'result' && message.subtype === 'success'), JSON.stringify(messages));
    assert.deepEqual(prompts, ['shield'], JSON.stringify(provider.results.at(-1)));
    for (const result of provider.results) {
      assert.ok(JSON.stringify(result).length < 24 * 1024, 'review output must fit inline');
      assert.doesNotMatch(JSON.stringify(result), /tool-results\/|persisted-output|output too large/i);
    }
    const after = JSON.parse(await readFile(f.statePath, 'utf8'));
    assert.equal(after.proposals.at(-1).status, behavior === 'allow' ? 'applied' : 'pending');
    if (behavior === 'allow') assert.notEqual(after.policy_revision, before.policy_revision);
    else assert.deepEqual(after.active, before.active);
    const events = await f.events();
    assert.equal(events.some(event => event.details?.operation?.kind === 'shell'), false);
  });
}
