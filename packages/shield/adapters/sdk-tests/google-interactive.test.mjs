import test from 'node:test';
import assert from 'node:assert/strict';
import { spawn, execFile } from 'node:child_process';
import { promisify } from 'node:util';
import { once } from 'node:events';
import { setTimeout as delay } from 'node:timers/promises';
import { mkdir, writeFile, readFile } from 'node:fs/promises';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { enrolledRuntime } from '../fixtures/runtime.mjs';
import { geminiPolicyChangeSteps, policyChange, reviewStatus } from './policy-proposal.mjs';
import { geminiProvider } from './gemini-provider.mjs';

const exec = promisify(execFile);
const helper = fileURLToPath(new URL('./claude-terminal.py', import.meta.url));
const observer = fileURLToPath(new URL('./mcp-observer.mjs', import.meta.url));
const unstyle = text => text.replace(/\x1b\][^\x07]*(?:\x07|\x1b\\)/g, '')
  .replace(/\x1b\[[0-?]*[ -/]*[@-~]/g, '');

async function terminal(t, argv, cwd, env) {
  const child = spawn('python3', [helper], { stdio: ['pipe', 'pipe', 'pipe'] });
  const closed = once(child, 'close');
  let text = '', pending = '', errors = '', exited = false;
  child.on('close', () => { exited = true; });
  child.stdout.on('data', bytes => {
    pending += bytes;
    let index;
    while ((index = pending.indexOf('\n')) !== -1) {
      const frame = JSON.parse(pending.slice(0, index));
      pending = pending.slice(index + 1);
      text += frame.text ?? '';
    }
  });
  child.stderr.on('data', bytes => { errors += bytes; });
  const send = input => child.stdin.write(`${JSON.stringify({ input })}\n`);
  child.stdin.write(`${JSON.stringify({ argv, cwd, env })}\n`);
  t.after(async () => {
    if (!exited) child.stdin.write(`${JSON.stringify({ stop: true })}\n`);
    await closed;
  });
  return {
    send,
    async stop() {
      if (!exited) child.stdin.write(`${JSON.stringify({ stop: true })}\n`);
      await closed;
    },
    text: () => unstyle(text),
    async wait(condition) {
      const deadline = Date.now() + 35000;
      while (!await condition(unstyle(text))) {
        assert.ok(!exited && Date.now() < deadline && !t.signal.aborted,
          `Terminal stopped or timed out: ${errors}\n${unstyle(text).slice(-10000)}`);
        await delay(25);
      }
    },
  };
}

async function geminiEnvironment(root, provider) {
  const home = join(root, 'gemini-home');
  await mkdir(join(home, '.gemini'), { recursive: true });
  await writeFile(join(home, '.gemini/settings.json'), JSON.stringify({
    telemetry: { enabled: false }, privacy: { usageStatisticsEnabled: false },
    security: { auth: { selectedType: 'gemini-api-key' } }, ui: { showBanner: false },
  }));
  return { ...process.env, TERM: 'xterm-256color', NO_COLOR: '1',
    GEMINI_CLI_HOME: home, GEMINI_CLI_TRUST_WORKSPACE: 'true',
    GOOGLE_GEMINI_BASE_URL: provider.url, GEMINI_API_KEY: 'acme-local-test-key',
    GOOGLE_GENAI_USE_GCA: 'false', GOOGLE_GENAI_USE_VERTEXAI: 'false',
    GEMINI_TELEMETRY_ENABLED: 'false', HTTP_PROXY: '', HTTPS_PROXY: '', ALL_PROXY: '', NO_PROXY: '127.0.0.1,localhost' };
}

async function interactiveRuntime(t) {
  const stops = [];
  // Register process teardown before the enrolled fixture removes its files.
  t.after(async () => { for (const stop of stops) await stop(); });
  const f = await enrolledRuntime(t);
  f.terminal = async (argv, env) => {
    const ui = await terminal(t, argv, f.root, env);
    stops.push(ui.stop);
    return ui;
  };
  return f;
}

for (const agent of ['gemini', 'agy']) {
  for (const action of agent === 'gemini' ? ['unavailable'] : ['modern cancellation', 'classic cancellation']) {
    test(`interactive ${agent}: policy form ${action}`, {
      timeout: 60000, skip: agent === 'agy' && process.env.SHIELD_LIVE_AGY !== '1',
    }, async t => {
      const f = await interactiveRuntime(t);
      await f.operator(['mcp', 'setup', '--agent', agent]);
      const state = async () => JSON.parse(await readFile(f.statePath, 'utf8'));
      const before = await f.revision();
      let env = { ...process.env, TERM: 'xterm-256color', NO_COLOR: '1' };
      let argv, provider;
      if (agent === 'gemini') {
        provider = await geminiProvider(t, geminiPolicyChangeSteps());
        env = await geminiEnvironment(f.root, provider);
        argv = [(await exec('which', ['gemini'])).stdout.trim(), '--skip-trust', '--approval-mode', 'yolo',
          '--model', 'gemini-2.5-flash', '--prompt-interactive', 'Perform the Acme fixture tool exactly once.'];
      } else {
        const path = join(f.root, '.agents/mcp_config.json');
        const config = JSON.parse(await readFile(path, 'utf8'));
        const entry = config.mcpServers.shield;
        const trace = join(f.root, 'mcp-trace.jsonl');
        entry.args = [observer, entry.command, ...entry.args];
        entry.command = process.execPath;
        entry.env = { ACME_MCP_TRACE: trace, ACME_MCP_CLASSIC_ONLY: action.startsWith('classic') ? '1' : '0' };
        await writeFile(path, JSON.stringify(config));
        argv = [(await exec('which', ['agy'])).stdout.trim(), '--effort', 'low', '--mode', 'auto'];
      }
      const ui = await f.terminal(argv, env);
      if (agent === 'gemini') {
        // The first result is the proposal; the second is the apply attempt.
        await ui.wait(text => /Select Theme/.test(text) || text.replace(/\s/g, '').includes('ApprovethisShieldpolicychange?') || (provider?.results.length > 1));
        if (/Select Theme/.test(ui.text())) ui.send('\r');
        await ui.wait(() => provider.results.length > 1);
        assert.match(JSON.stringify(provider.results.slice(1)), /approval_unavailable/);
        assert.equal(await f.revision(), before);
        assert.equal((await state()).proposals.at(-1).status, reviewStatus.unavailable);
        return;
      }
      await ui.wait(text => text.includes('Do you trust the contents of this project?') || text.includes('for shortcuts'));
      if (ui.text().includes('Do you trust the contents of this project?')) ui.send('\r');
      await ui.wait(text => text.includes('for shortcuts'));
      ui.send('/mcp\r');
      await ui.wait(text => text.includes('MCP Servers'));
      await ui.wait(text => /shield[^\n]*(connected|ready|tools)/i.test(text));
      let offset = ui.text().length;
      ui.send('\x1b');
      await ui.wait(text => text.slice(offset).includes('for shortcuts'));
      if (ui.text().slice(offset).includes('esc to dismiss')) {
        offset = ui.text().length;
        ui.send('\x1b');
        await ui.wait(text => text.slice(offset).includes('for shortcuts'));
      }
      ui.send(`Call the shield MCP policy_propose tool once with arguments ${JSON.stringify({ change: policyChange })}, then call policy_apply exactly once with the apply_arguments it returns. Do not run other tools or retry. Stop after its response.`);
      await ui.wait(text => text.includes('Call the shield MCP policy_propose tool once'));
      ui.send('\r');
      const trace = async () => (await readFile(join(f.root, 'mcp-trace.jsonl'), 'utf8').catch(() => ''))
        .trim().split('\n').filter(Boolean).map(line => JSON.parse(line));
      await ui.wait(async () => (await trace()).some(frame => frame.errorCode === 'approval_declined'));
      assert.ok((await trace()).some(frame => frame.responseAction === 'cancel' || frame.inputActions?.confirmation === 'cancel'));
      const after = await state();
      assert.equal(after.proposals.at(-1).status, reviewStatus.cancel);
      assert.equal(await f.revision(), before);
    });
  }
}

for (const action of ['accept', 'cancel']) {
  test(`interactive Gemini native Ask ${action}`, { timeout: 60000 }, async t => {
    const f = await interactiveRuntime(t);
    await f.operator(['mcp', 'setup', '--agent', 'gemini']);
    const provider = await geminiProvider(t, [{ name: 'read_file', input: { file_path: f.asked } }]);
    const env = await geminiEnvironment(f.root, provider);
    const executable = (await exec('which', ['gemini'])).stdout.trim();
    const ui = await f.terminal([executable, '--skip-trust', '--approval-mode', 'yolo',
      '--model', 'gemini-2.5-flash', '--prompt-interactive', 'Perform the Acme fixture tool exactly once.'], env);
    await ui.wait(text => /Select Theme/.test(text) || /approval required/i.test(text));
    if (/Select Theme/.test(ui.text())) ui.send('\r');
    await ui.wait(text => /approval required/i.test(text) && text.includes('Allow once'));
    assert.equal(provider.results.length, 0);
    ui.send(action === 'accept' ? '\r' : '\x1b');
    await ui.wait(text => action === 'accept' ? provider.results.length > 0 : text.includes('Request cancelled.'));
    assert.equal(JSON.stringify(provider.results).includes('ACME_ASKED_TEXT'), action === 'accept');
  });
}

for (const action of ['acceptance', 'refusal']) {
test(`interactive Antigravity native Ask ${action}`, { timeout: 60000, skip: process.env.SHIELD_LIVE_AGY !== '1' }, async t => {
  const f = await interactiveRuntime(t);
  await f.operator(['mcp', 'setup', '--agent', 'agy']);
  const executable = (await exec('which', ['agy'])).stdout.trim();
  const ui = await f.terminal([executable, '--effort', 'low', '--prompt-interactive',
    `Read ${JSON.stringify(f.asked)} exactly once with read_file. Return its exact contents. Do not use other tools or retry.`],
    { ...process.env, TERM: 'xterm-256color', NO_COLOR: '1' });
  await ui.wait(text => text.includes('Do you trust the contents of this project?') || text.includes('for shortcuts'));
  if (ui.text().includes('Do you trust the contents of this project?')) ui.send('\r');
  await ui.wait(text => text.includes('for shortcuts'));
  const offset = ui.text().length;
  await ui.wait(text => text.slice(offset).includes('Yes, allow access') && text.slice(offset).includes('Shield:'));
  assert.ok(!ui.text().includes('ACME_ASKED_TEXT'));
  const answered = ui.text().length;
  if (action === 'refusal') {
    ui.send('\x1b[B');
    await ui.wait(text => text.slice(answered).includes('> 2. No, deny access'));
  }
  ui.send('\r');
  await ui.wait(text => action === 'acceptance' ? text.includes('ACME_ASKED_TEXT')
    : text.slice(answered).includes('User declined the tool call'));
  assert.equal(ui.text().includes('ACME_ASKED_TEXT'), action === 'acceptance');
});
}
