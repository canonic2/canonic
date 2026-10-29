// Host projection capture: what each real host sends its model for Shield's tools.
// A local scripted model records the requests, so no model usage is spent.
// Usage: node benchmark/projection.mjs [--runs <directory>]
import { query } from '@anthropic-ai/claude-agent-sdk';
import { spawn } from 'node:child_process';
import { createServer } from 'node:http';
import { mkdir, writeFile } from 'node:fs/promises';
import { homedir } from 'node:os';
import { join } from 'node:path';
import { createInterface } from 'node:readline';
import { parseArgs } from 'node:util';
import { scriptedProvider } from '../claude-provider.mjs';
import { createRun, digestFile, exec, initialize, which } from './lib/project.mjs';

const { values: options } = parseArgs({ options: { runs: { type: 'string', default: join(homedir(), 'ShieldSessions/runs') } } });
const shield = await which('shield');
const call = { session_id: 'acme-projection-session' };
const cleanups = [];
const owner = { after: fn => cleanups.push(fn) };
const fixture = { project: 'acme', seed: { 'README.md': '# Acme\n' }, tools: {} };

async function claude() {
  const run = await createRun(options.runs, 'projection-claude', fixture);
  await initialize(shield, run, 'claude');
  const provider = await scriptedProvider(owner, [{ name: 'mcp__shield__suggestion_list', input: call }]);
  const home = join(run.root, 'claude-home');
  await mkdir(home, { mode: 0o700 });
  const env = { ...process.env, CLAUDE_CONFIG_DIR: home, ANTHROPIC_API_KEY: 'acme-local-test-key', ANTHROPIC_BASE_URL: provider.url };
  const messages = [];
  for await (const message of query({ prompt: 'Acme projection capture.', options: {
    cwd: run.project, env, pathToClaudeCodeExecutable: await which('claude'),
    systemPrompt: { type: 'preset', preset: 'claude_code' }, settingSources: ['user', 'project', 'local'],
    settings: { disableClaudeAiConnectors: true },
    canUseTool: async (_tool, input) => ({ behavior: 'allow', updatedInput: input }),
  } })) messages.push(message);
  const first = provider.requests.find(body => body.tools?.length);
  return {
    host_cli: (await exec('claude', ['--version'])).stdout.trim(),
    tools: (first?.tools ?? []).filter(tool => tool.name?.startsWith('mcp__shield__'))
      .map(({ name, description, input_schema, defer_loading }) => ({ name, description, input_schema, defer_loading })),
    other_tool_names: (first?.tools ?? []).filter(tool => !tool.name?.startsWith('mcp__shield__')).map(tool => tool.name),
    result_blocks: provider.results,
    errors: messages.filter(message => message.type === 'result' && message.is_error),
  };
}

async function codex(codeMode) {
  const run = await createRun(options.runs, `projection-codex${codeMode ? '-code-mode' : ''}`, fixture);
  await initialize(shield, run, 'codex');
  const requests = [];
  let count = 0;
  const server = createServer(async (request, response) => {
    let bytes = '';
    for await (const chunk of request) bytes += chunk;
    const body = JSON.parse(bytes || '{}');
    requests.push(body);
    const namespace = body.tools?.find(tool => tool.type === 'namespace' && tool.name === 'mcp__shield');
    const item = count++ === 0 && namespace && !codeMode
      ? { type: 'function_call', id: 'fc_acme', call_id: 'call_acme', namespace: namespace.name, name: 'suggestion_list', arguments: JSON.stringify(call) }
      : { type: 'message', id: 'msg_acme', role: 'assistant', status: 'completed', content: [{ type: 'output_text', text: 'Acme capture finished.', annotations: [] }] };
    const result = { id: `resp_acme_${count}`, object: 'response', created_at: 0, model: body.model, status: 'completed', output: [item], usage: { input_tokens: 10, output_tokens: 10, total_tokens: 20 } };
    response.writeHead(200, { 'content-type': 'text/event-stream' });
    for (const event of [{ type: 'response.created', response: { ...result, status: 'in_progress', output: [] } },
      { type: 'response.output_item.added', output_index: 0, item }, { type: 'response.output_item.done', output_index: 0, item },
      { type: 'response.completed', response: result }]) response.write(`data: ${JSON.stringify(event)}\n\n`);
    response.end();
  });
  await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
  cleanups.push(() => new Promise(resolve => { server.closeAllConnections(); server.close(resolve); }));
  const home = join(run.root, 'codex-home');
  await mkdir(home, { mode: 0o700 });
  await writeFile(join(home, 'config.toml'), `model = "acme-model"\nmodel_provider = "acme"\n${codeMode ? '[features]\ncode_mode = true\n' : ''}[model_providers.acme]\nname = "Acme local capture"\nbase_url = "http://127.0.0.1:${server.address().port}/v1"\nwire_api = "responses"\nrequires_openai_auth = false\n[projects.${JSON.stringify(run.project)}]\ntrust_level = "trusted"\n`);
  const child = spawn(await which('codex'), ['app-server'], { cwd: run.project, env: { ...process.env, CODEX_HOME: home }, stdio: ['pipe', 'pipe', 'inherit'] });
  const pending = new Map();
  let id = 0;
  const ready = Promise.withResolvers();
  const done = Promise.withResolvers();
  createInterface({ input: child.stdout }).on('line', line => {
    const message = JSON.parse(line);
    if (message.id !== undefined && !message.method) pending.get(message.id)?.(message);
    else if (message.id !== undefined) child.stdin.write(`${JSON.stringify({ id: message.id, error: { code: -32601, message: 'capture' } })}\n`);
    else if (message.method === 'mcpServer/startupStatus/updated' && message.params.name === 'shield' && message.params.status === 'ready') ready.resolve();
    else if (message.method === 'turn/completed') done.resolve();
  });
  const rpc = (method, params) => new Promise(resolve => { pending.set(++id, resolve); child.stdin.write(`${JSON.stringify({ id, method, params })}\n`); });
  await rpc('initialize', { clientInfo: { name: 'shield-projection', version: '1' }, capabilities: { experimentalApi: true } });
  child.stdin.write(`${JSON.stringify({ method: 'initialized' })}\n`);
  const { result } = await rpc('thread/start', { cwd: run.project, approvalPolicy: 'never', sandbox: 'read-only', ephemeral: true, config: { bypass_hook_trust: true } });
  await ready.promise;
  await rpc('turn/start', { threadId: result.thread.id, input: [{ type: 'text', text: 'Acme projection capture.' }] });
  await Promise.race([done.promise, new Promise(resolve => setTimeout(resolve, 30000))]);
  child.kill('SIGTERM');
  const tools = requests[0]?.tools ?? [];
  const namespace = tools.find(tool => tool.type === 'namespace' && tool.name === 'mcp__shield');
  return {
    host_cli: (await exec('codex', ['--version'])).stdout.trim(), code_mode: codeMode,
    tools: namespace?.tools ?? null,
    top_level_tools: tools.map(tool => ({ type: tool.type, name: tool.name, description: tool.type === 'namespace' ? undefined : tool.description?.slice(0, 4000) })),
    result_items: requests.slice(1).flatMap(body => (body.input ?? []).filter(item => item.type === 'function_call_output')),
  };
}

try {
  const capture = { at: new Date().toISOString(), shield_sha256: await digestFile(shield),
    shield_version: (await exec(shield, ['--version'])).stdout.trim(),
    claude: await claude(), codex: await codex(false), codex_code_mode: await codex(true) };
  const path = join(options.runs, `projection-${capture.at.replace(/[-:]/g, '').replace(/\..+/, '')}.json`);
  await writeFile(path, `${JSON.stringify(capture, null, 2)}\n`);
  process.stdout.write(`${path}\n`);
} finally {
  for (const cleanup of cleanups.reverse()) await cleanup();
}
