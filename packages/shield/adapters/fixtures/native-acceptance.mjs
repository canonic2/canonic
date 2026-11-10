// Owned local-provider acceptance resources. No account environment is inherited.
import assert from 'node:assert/strict';
import { createServer } from 'node:http';
import { mkdir, symlink } from 'node:fs/promises';
import { join } from 'node:path';
import { binary, fixtureEnvironment, home } from './runtime.mjs';
import { command as ownedCommand } from './native-command.mjs';

const paths = {
  bin: join(home, 'native-acceptance-bin'), tmp: join(home, 'native-acceptance-tmp'),
  copilot: join(home, 'native-acceptance-copilot'), claude: join(home, 'native-acceptance-claude'),
};
if (process.platform === 'darwin' && ['copilot', 'opencode'].includes(process.env.SHIELD_NATIVE_ACCEPTANCE_HOST)) {
  for (const path of Object.values(paths)) await mkdir(path, { recursive: true });
  await symlink(binary, join(paths.bin, 'shield'));
}

export function environment(host, provider) {
  const env = { ...fixtureEnvironment, PATH: `${paths.bin}:${process.env.PATH ?? '/usr/bin:/bin'}`,
    TMPDIR: paths.tmp, LANG: 'C', TERM: 'dumb', NO_COLOR: '1', CI: 'true', SHELL: '/bin/bash',
    HTTP_PROXY: '', HTTPS_PROXY: '', ALL_PROXY: '', NO_PROXY: '127.0.0.1,localhost',
    CLAUDE_CONFIG_DIR: paths.claude };
  if (host === 'copilot') return { ...env, COPILOT_HOME: paths.copilot, COPILOT_OFFLINE: 'true',
    COPILOT_PROVIDER_BASE_URL: provider ?? 'http://127.0.0.1:1/v1', COPILOT_PROVIDER_TYPE: 'openai',
    COPILOT_PROVIDER_WIRE_API: 'completions', COPILOT_PROVIDER_API_KEY: 'acme-dummy-key',
    OPENAI_API_KEY: 'acme-dummy-key', COPILOT_MODEL: 'gpt-4.1', USE_TGREP: 'false' };
  return { ...env, DO_NOT_TRACK: '1', OPENCODE_DISABLE_AUTOUPDATE: 'true',
    OPENCODE_DISABLE_MODELS_FETCH: 'true', OPENCODE_DISABLE_DEFAULT_PLUGINS: 'true',
    OPENCODE_DISABLE_LSP_DOWNLOAD: 'true', OPENCODE_DISABLE_EXTERNAL_SKILLS: 'true',
    OPENCODE_DISABLE_CLAUDE_CODE: 'true' };
}

/** Same-group cleanup retains and reaps the leader before returning. */
export function command(program, args, options = {}) {
  return ownedCommand(program, args, { cwd: home, ...options });
}

const checked = new Map();
export function preflight(host) {
  if (!checked.has(host)) checked.set(host, (async () => {
    assert.equal(process.platform, 'darwin', 'This acceptance target is the selected Mac only');
    const env = environment(host);
    const version = await command(host, host === 'copilot' ? ['--no-auto-update', '--disable-client-telemetry', '--version'] : ['--version'], { env, timeoutMs: 15_000 });
    assert.equal(version.code, 0, version.stderr);
    assert.match(version.stdout, host === 'copilot' ? /\b1\.0\.105\b/ : /\b1\.14\.27\b/,
      'Installed isolated runtime differs from the pinned evidence; stop and review its contract');
    if (host === 'copilot') {
      const options = await command(host, ['--help'], { env, timeoutMs: 15_000 });
      assert.equal(options.code, 0, options.stderr);
      assert.match(options.stdout, /--disable-mcp-server\s+<[^>]+>/,
        'Focused native file callbacks require the documented per-server disable option');
      for (const [topic, required] of [['environment', /COPILOT_OFFLINE/], ['providers', /COPILOT_PROVIDER_BASE_URL/]]) {
        const help = await command(host, ['help', topic], { env, timeoutMs: 15_000 });
        assert.equal(help.code, 0, help.stderr);
        assert.match(help.stdout, required, 'Offline/local provider route is a prerequisite');
      }
    }
    return version.stdout.trim();
  })());
  return checked.get(host);
}

/** A deterministic loopback Chat Completions endpoint, not a model service. */
export async function provider(t, host, file) {
  const toolName = host === 'copilot' ? 'view' : 'read';
  const args = host === 'copilot' ? { path: file } : { filePath: file };
  const result = { issued: false, requests: 0, results: [], failure: undefined };
  const server = createServer(async (request, response) => {
    try {
      assert.equal(request.method, 'POST');
      assert.match(request.url, /^\/v1\/chat\/completions$/);
      assert.ok(++result.requests <= 16, 'Bound local provider traffic');
      const chunks = []; let size = 0;
      for await (const bytes of request) {
        size += bytes.length;
        assert.ok(size <= 4 * 1024 * 1024, 'Bound provider input');
        chunks.push(bytes);
      }
      const body = JSON.parse(Buffer.concat(chunks));
      for (const message of body.messages ?? []) if (message.role === 'tool') {
        assert.ok(Buffer.byteLength(JSON.stringify(message)) <= 256 * 1024, 'Bound native tool-result evidence');
        result.results.push(message);
      }
      const tool = body.tools?.find(tool => tool.function?.name === toolName);
      let payload = { role: 'assistant', content: 'Native fixture complete.' };
      if (!result.issued && tool) {
        const schema = tool.function.parameters;
        for (const field of Object.keys(args)) assert.ok(schema?.properties?.[field], 'Pinned native file schema changed');
        for (const field of schema.required ?? []) assert.ok(Object.hasOwn(args, field), 'Native schema requires an unverified field');
        result.issued = true;
        payload = { role: 'assistant', content: null, tool_calls: [{ id: 'native-acceptance-call', type: 'function',
          function: { name: toolName, arguments: JSON.stringify(args) } }] };
      }
      if (body.stream) {
        response.writeHead(200, { 'Content-Type': 'text/event-stream' });
        const delta = payload.tool_calls ? { ...payload, tool_calls: payload.tool_calls.map((call, index) => ({ ...call, index })) } : payload;
        for (const [part, reason] of [[delta, null], [{}, payload.tool_calls ? 'tool_calls' : 'stop']]) {
          response.write(`data: ${JSON.stringify({ id: 'fixture', object: 'chat.completion.chunk', created: 1,
            model: 'fixture', choices: [{ index: 0, delta: part, finish_reason: reason }] })}\n\n`);
        }
        response.end('data: [DONE]\n\n');
      } else {
        response.writeHead(200, { 'Content-Type': 'application/json' });
        response.end(JSON.stringify({ id: 'fixture', object: 'chat.completion', created: 1, model: 'fixture',
          choices: [{ index: 0, message: payload, finish_reason: payload.tool_calls ? 'tool_calls' : 'stop' }],
          usage: { prompt_tokens: 1, completion_tokens: 1, total_tokens: 2 } }));
      }
    } catch (error) { result.failure = error; response.writeHead(500); response.end('Local fixture refused invalid traffic'); }
  });
  await new Promise((resolve, reject) => { server.once('error', reject); server.listen(0, '127.0.0.1', resolve); });
  t.after(async () => { server.closeAllConnections(); await new Promise(resolve => server.close(resolve)); });
  return { result, url: `http://127.0.0.1:${server.address().port}/v1` };
}

export function hostArguments(host) {
  return host === 'copilot'
    ? ['--no-auto-update', '--disable-client-telemetry', '--disable-builtin-mcps', '--trust-folder', '--allow-all-tools',
      '--disable-mcp-server', 'shield',
      '--no-custom-instructions', '--no-ask-user', '--output-format', 'json', '-p', 'Perform the single requested native file read once, then finish.']
    : ['run', '--format', 'json', '--model', 'fixture/fixture', 'Perform the single requested native file read once, then finish.'];
}

export function openCodeConfig(url) {
  return { shell: '/bin/bash', model: 'fixture/fixture', small_model: 'fixture/fixture', enabled_providers: ['fixture'],
    provider: { fixture: { npm: '@ai-sdk/openai-compatible', name: 'Owned loopback fixture', options: { baseURL: url, apiKey: 'acme-dummy-key' },
      models: { fixture: { name: 'Fixture', tool_call: true, limit: { context: 64_000, output: 1000 } } } } }, permission: 'allow' };
}
