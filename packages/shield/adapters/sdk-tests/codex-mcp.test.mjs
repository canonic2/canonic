// Real Codex host approval behavior with Shield's generated project config.
import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdir, writeFile, readFile } from 'node:fs/promises';
import { join } from 'node:path';
import { createServer } from 'node:http';
import { execFile } from 'node:child_process';
import { promisify } from 'node:util';
import { enrolledRuntime } from '../fixtures/runtime.mjs';
import { approvalHost } from './codex-host.mjs';
import { policyChange, applyArguments } from './policy-proposal.mjs';
const exec = promisify(execFile);

for (const hooks of [true, false]) {
for (const approval of ['proposal', 'accept', 'decline', 'never']) {
test(`Codex Shield MCP ${approval} with native hooks ${hooks ? 'enabled' : 'disabled'}`, { timeout: 60000 }, async t => {
  let server;
  let removeProject;
  t.after(async () => {
    try {
      if (server?.listening) await new Promise((resolve, reject) => {
        server.close(error => error ? reject(error) : resolve());
        server.closeAllConnections();
      });
    } finally { await removeProject?.(); }
  });
  const f = await enrolledRuntime({ after: cleanup => { removeProject = cleanup; } });
  await exec('git', ['init', '-q', f.root]);
  await f.operator(['mcp', 'setup', '--agent', 'codex']);
  if (!hooks) {
    const path = join(f.root, '.codex/config.toml');
    await writeFile(path, `${await readFile(path, 'utf8')}\n[features]\nhooks = false\n`);
  }
  const home = join(f.root, 'codex-test-home');
  await mkdir(home);
  const proposal = { change: { kind: 'rule_add', rule: {
    id: 'acme-proposed-read', decision: 'allow', reason: 'Acme proposed inspection',
    conditions: { domain: 'argv', executable: 'acme', arguments: { kind: 'exact', values: ['inspect'] } },
  } }, idempotency_key: 'acme-mcp-approval', agent_reason: 'Acme MCP autoapproval fixture' };
  const applying = approval !== 'proposal';
  // Applying proposes on the same connection first, since only it may apply.
  let applied;
  const calls = applying
    ? [['policy_propose', () => ({ change: policyChange })],
      ['policy_apply', body => (applied = applyArguments(body.input?.filter(item => item.type === 'function_call_output')))]]
    : [['policy_propose', () => proposal]];
  const before = JSON.parse(await readFile(f.statePath, 'utf8'));
  const revision = await f.revision();
  let count = 0;
  const requests = [];
  server = createServer(async (req, res) => {
    let bytes = '';
    for await (const chunk of req) bytes += chunk;
    const body = JSON.parse(bytes || '{}');
    requests.push(body);
    const namespace = body.tools?.find(tool => tool.type === 'namespace' && tool.name === 'mcp__shield');
    const step = calls[count];
    const tool = step && namespace?.tools?.find(tool => tool.name === step[0]);
    const item = step
      ? { type: 'function_call', id: `fc_acme_${count}`, call_id: `call_acme_${count}`, namespace: namespace?.name, name: tool?.name ?? (namespace ? step[0] : `mcp__shield__${step[0]}`), arguments: JSON.stringify(step[1](body)) }
      : { type: 'message', id: 'msg_acme', role: 'assistant', status: 'completed', content: [{ type: 'output_text', text: 'Acme proposal submitted.', annotations: [] }] };
    count++;
    const response = { id: `resp_acme_${count}`, object: 'response', created_at: 0, model: body.model, status: 'completed', output: [item], usage: { input_tokens: 10, output_tokens: 10, total_tokens: 20 } };
    res.writeHead(200, { 'content-type': 'text/event-stream' });
    for (const event of [
      { type: 'response.created', response: { ...response, status: 'in_progress', output: [] } },
      { type: 'response.output_item.added', output_index: 0, item },
      { type: 'response.output_item.done', output_index: 0, item },
      { type: 'response.completed', response },
    ]) res.write(`data: ${JSON.stringify(event)}\n\n`);
    res.end();
  });
  await new Promise((resolve, reject) => { server.once('error', reject); server.listen(0, '127.0.0.1', resolve); });
  await writeFile(join(home, 'config.toml'), `model = "acme-model"\nmodel_provider = "acme"\n[model_providers.acme]\nname = "Acme local fixture"\nbase_url = "http://127.0.0.1:${server.address().port}/v1"\nwire_api = "responses"\nrequires_openai_auth = false\n[projects.${JSON.stringify(f.root)}]\ntrust_level = "trusted"\n`);
  // Any unexpected MCP approval RPC fails this real host fixture; shell requests
  // are declined and counted rather than silently accepted by the client.
  const prompting = applying && approval !== 'never';
  const result = await approvalHost(f.root, home, 'decline', prompting ? approval : undefined,
    { approvalPolicy: approval === 'never' ? 'never' : 'on-request', waitForShield: true });
  assert.deepEqual(result.approvals, []);
  assert.equal(result.mcpApprovals.length, prompting ? 1 : 0, result.stdout + result.stderr);
  if (prompting) {
    const prompt = JSON.stringify(result.mcpApprovals);
    assert.match(prompt, /policy_apply/);
    const description = [applied.change, applied.details ?? ''].join('\n');
    for (const line of description.split('\n').filter(Boolean)) {
      assert.ok(prompt.includes(line), prompt);
    }
    assert.ok(prompt.includes(applied.reference), prompt);
    assert.doesNotMatch(prompt, /candidate_digest|proposal_id|"changes"/);
    assert.match(prompt, /acme-host-approved/);
  }
  assert.doesNotMatch(result.stdout + result.stderr, /Hook failed|unsupported permissionDecision/i);
  const outputs = requests.flatMap(r => r.input ?? []).filter(item => item.type === 'function_call_output')
    .map(item => typeof item.output === 'string' ? item.output : JSON.stringify(item.output)).join('\n');
  if (!['decline', 'never'].includes(approval)) assert.match(outputs, /proposal_id/, outputs);
  if (approval === 'never') assert.match(outputs, /requires approval|approval policy is never|unsupported call/i);
  const after = JSON.parse(await readFile(f.statePath, 'utf8'));
  assert.equal(after.proposals.length, before.proposals.length + 1);
  // Codex asks before calling the tool, so a declined call never reaches Shield.
  assert.equal(after.proposals.at(-1).status, approval === 'accept' ? 'applied' : 'pending');
  if (approval === 'accept') {
    assert.equal(after.proposals.at(-1).operator.mechanism, 'host-tool-permission');
    assert.notEqual(await f.revision(), revision);
  } else {
    assert.equal(after.proposals.at(-1).operator, null);
    assert.equal(await f.revision(), revision);
  }
});
}
}
