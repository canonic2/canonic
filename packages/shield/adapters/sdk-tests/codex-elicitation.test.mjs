// Real Codex with a scripted model and MCP peer: no model usage or user profile.
import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtemp, mkdir, writeFile, readFile, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { createServer } from 'node:http';
import { approvalHost } from './codex-host.mjs';

test('Codex forwards a standard multi-select MCP form to its app-server client', { timeout: 60000 }, async t => {
  const root = await mkdtemp(join(tmpdir(), 'acme-codex-form-'));
  const home = join(root, 'home');
  await mkdir(home);
  let count = 0;
  const provider = createServer(async (request, response) => {
    let bytes = '';
    for await (const chunk of request) bytes += chunk;
    const body = JSON.parse(bytes);
    const namespace = body.tools?.find(tool => tool.type === 'namespace' && tool.name === 'mcp__shield');
    const item = count++ === 0
      ? { type: 'function_call', id: 'fc_acme', call_id: 'call_acme', namespace: namespace?.name, name: namespace ? 'acme_form' : 'mcp__shield__acme_form', arguments: '{}' }
      : { type: 'message', id: 'msg_acme', role: 'assistant', status: 'completed', content: [{ type: 'output_text', text: 'Acme done.', annotations: [] }] };
    const result = { id: `resp_acme_${count}`, object: 'response', created_at: 0, model: body.model, status: 'completed', output: [item], usage: { input_tokens: 10, output_tokens: 10, total_tokens: 20 } };
    response.writeHead(200, { 'content-type': 'text/event-stream' });
    for (const event of [
      { type: 'response.created', response: { ...result, status: 'in_progress', output: [] } },
      { type: 'response.output_item.added', output_index: 0, item },
      { type: 'response.output_item.done', output_index: 0, item },
      { type: 'response.completed', response: result },
    ]) response.write(`data: ${JSON.stringify(event)}\n\n`);
    response.end();
  });
  t.after(async () => {
    if (provider.listening) await new Promise(resolve => { provider.close(resolve); provider.closeAllConnections(); });
    await rm(root, { recursive: true, force: true });
  });
  await new Promise(resolve => provider.listen(0, '127.0.0.1', resolve));
  const schema = { type: 'object', properties: {
    manager: { type: 'string', title: 'manager', description: 'Build manager', enum: ['make', 'just'] },
    regions: { type: 'array', title: 'regions', description: 'Allowed regions', items: { type: 'string', enum: ['east', 'west', 'north'] }, minItems: 1, maxItems: 2 },
    enabled: { type: 'boolean', title: 'enabled', description: 'Enable checks' },
    label: { type: 'string', title: 'label', description: 'Project label', minLength: 1, maxLength: 4096 },
  }, required: ['manager', 'regions', 'enabled', 'label'] };
  const question = join(root, 'question.json');
  const trace = join(root, 'trace.json');
  await writeFile(question, JSON.stringify({ mode: 'form', message: 'Choose Acme settings', requestedSchema: schema }));
  await writeFile(join(home, 'config.toml'), `model = "acme-model"\nmodel_provider = "acme"\n[model_providers.acme]\nname = "Acme local fixture"\nbase_url = "http://127.0.0.1:${provider.address().port}/v1"\nwire_api = "responses"\nrequires_openai_auth = false\n[projects.${JSON.stringify(root)}]\ntrust_level = "trusted"\n[mcp_servers.shield]\ncommand = ${JSON.stringify(process.execPath)}\nargs = ${JSON.stringify([fileURLToPath(new URL('./elicitation-server.mjs', import.meta.url)), question, trace])}\n[mcp_servers.shield.tools.acme_form]\napproval_mode = "approve"\n`);
  const result = await approvalHost(root, home, 'decline', 'cancel', { waitForShield: true });
  const messages = JSON.parse(await readFile(trace, 'utf8'));
  t.diagnostic(`Client requested MCP ${messages.find(message => message.method === 'initialize')?.params.protocolVersion}; fixture selected 2025-11-25.`);
  assert.equal(result.mcpApprovals.length, 1, JSON.stringify(messages));
  assert.deepEqual(result.mcpApprovals[0].params.requestedSchema, schema);
});
