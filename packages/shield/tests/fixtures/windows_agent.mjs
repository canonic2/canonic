// Native host acceptance against owned loopback providers; no external model calls.
import assert from 'node:assert/strict';
import { readFile, writeFile, mkdir, mkdtemp, rm } from 'node:fs/promises';
import { join } from 'node:path';
import { createServer } from 'node:http';
import { execFile } from 'node:child_process';
import { promisify } from 'node:util';
import { scriptedProvider } from './claude-provider.mjs';

const exec = promisify(execFile);
const [root, agent, packages, shield] = process.argv.slice(2);
assert.equal(process.platform, 'win32');
assert.ok(root && packages && ['claude', 'codex'].includes(agent));
const cleanup = [];
const lifetime = { after(fn) { cleanup.push(fn); } };
try {
  if (agent === 'claude') {
    await exec('git', ['init', '-q', root]);
    const executable = join(packages, 'node_modules/@anthropic-ai/claude-code/bin/claude.exe');
    const settingsPath = join(root, '.claude/settings.local.json');
    const original = await readFile(settingsPath, 'utf8');
    const settings = JSON.parse(original);
    assert.ok(Array.isArray(settings.hooks.PreToolUse[0].hooks[0].args));
    assert.equal(settings.hooks.PreToolUse[0].hooks[0].onFailure, 'block');
    const reads = ['allowed', 'denied', 'asked'].map(name => ({
      name: 'Read', input: { file_path: join(root, `${name}.txt`) },
    }));
    const commands = ['git config --local acme.allowed present', 'git config --local acme.denied forbidden', 'git config --local acme.asked forbidden'].map(command => ({ name: 'PowerShell', input: { command } }));
    const provider = await scriptedProvider(lifetime, [...reads, ...commands]);
    const run = async provider => {
      const running = exec(executable, ['-p', 'Perform the Acme fixture reads in order.',
      '--output-format', 'json', '--setting-sources', 'project,local', '--tools', 'Read,PowerShell',
      '--allowedTools', 'Read,PowerShell', '--no-session-persistence', '--max-turns', '8'], {
      cwd: root, timeout: 45_000, maxBuffer: 2_000_000,
      env: { ...process.env, ANTHROPIC_API_KEY: 'acme-local-test-key', ANTHROPIC_BASE_URL: provider.url,
        CLAUDE_CONFIG_DIR: join(root, 'claude-fixture-home'), CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC: '1',
        CLAUDE_CODE_USE_POWERSHELL_TOOL: '1', CLAUDE_CODE_POWERSHELL_RESPECT_EXECUTION_POLICY: '1' },
      });
      running.child.stdin.end();
      return running;
    };
    await run(provider);
    const delivered = JSON.stringify(provider.results);
    assert.match(delivered, /ACME_ALLOWED_TEXT/);
    assert.doesNotMatch(delivered, /ACME_DENIED_TEXT|ACME_ASKED_TEXT/);
    assert.match(delivered, /Shield:/);
    assert.equal((await exec('git', ['-C', root, 'config', '--local', '--get-regexp', '^acme\\.'])).stdout.trim(), 'acme.allowed present');
    const failure = await scriptedProvider(lifetime, [{ name: 'Read', input: { file_path: join(root, 'allowed.txt') } }]);
    settings.hooks.PreToolUse[0].hooks[0].command = join(root, 'missing-shield.exe');
    try {
      await writeFile(settingsPath, JSON.stringify(settings));
      await run(failure);
      assert.doesNotMatch(JSON.stringify(failure.results), /ACME_ALLOWED_TEXT/);
      assert.match(JSON.stringify(failure.results), /hook|block|failed/i);
      const slow = join(root, 'slow-hook.mjs');
      await writeFile(slow, "setTimeout(() => console.log('{}'), 3000);\n");
      const timeout = await scriptedProvider(lifetime, [{ name: 'Read', input: { file_path: join(root, 'allowed.txt') } }]);
      settings.hooks.PreToolUse[0].hooks[0] = { type: 'command', command: process.execPath, args: [slow], onFailure: 'block', timeout: 1 };
      await writeFile(settingsPath, JSON.stringify(settings));
      await run(timeout);
      assert.doesNotMatch(JSON.stringify(timeout.results), /ACME_ALLOWED_TEXT/);
      assert.match(JSON.stringify(timeout.results), /hook|block|timed out/i);
    } finally { await writeFile(settingsPath, original); }
    await writeFile(join(root, 'agent-probe.json'), JSON.stringify({ agent, allowed: true, denied: true, unansweredAskWithheld: true, nativePowerShell: true, missingHookBlocked: true, timeoutHookBlocked: true }));
  } else {
    const executable = join(packages, 'node_modules/@openai/codex/bin/codex.js');
    await exec('git', ['init', '-q', root]);
    // Establish the side effect the prefix-covered Ask would have if dispatched.
    await exec('git', ['-C', root, 'config', '--local', 'acme.control', 'present', '--add']);
    assert.equal((await exec('git', ['-C', root, 'config', '--local', '--get-all', 'acme.control'])).stdout.trim(), 'present');
    await exec('git', ['-C', root, 'config', '--local', '--unset-all', 'acme.control']);
    const profiles = join(packages, 'profiles');
    await mkdir(profiles, { recursive: true });
    const home = await mkdtemp(join(profiles, 'codex-acme-'));
    cleanup.push(() => rm(home, { recursive: true, force: true }));
    const observer = join(root, 'observe-hook.mjs');
    await writeFile(observer, `import {writeFile} from 'node:fs/promises'; await writeFile(${JSON.stringify(join(root, 'observer-started'))},'started'); let bytes=''; for await(const chunk of process.stdin) bytes+=chunk; await writeFile(${JSON.stringify(join(root, 'hook-stdin.json'))},bytes); const input=JSON.parse(bytes); await writeFile(${JSON.stringify(join(root, 'hook-envelope.json'))},JSON.stringify({keys:Object.keys(input.tool_input).sort(),shell:input.tool_input.shell??null,source:input.tool_input.command})); console.log('{}');`);
    const hooksPath = join(root, '.codex/hooks.json');
    const original = await readFile(hooksPath, 'utf8');
    const hooks = JSON.parse(original);
    // Codex can launch hooks through its selected PowerShell, rather than
    // cmd.exe. Use the same cross-shell encoded launch form as Shield itself;
    // quoted executable paths alone are not executable PowerShell expressions.
    const shell = hooks.hooks.PreToolUse[0].hooks[0].command.split(' -NoLogo ')[0];
    assert.match(shell, /^[A-Za-z]:\/[A-Za-z0-9/._-]+\/powershell\.exe$/i);
    const literal = text => `'${text.replaceAll("'", "''")}'`;
    const script = `& ${literal(process.execPath)} ${literal(observer)}; if ($LASTEXITCODE -ne 0) { exit 2 }`;
    const command = `${shell} -NoLogo -NoProfile -NonInteractive -EncodedCommand ${Buffer.from(script, 'utf16le').toString('base64')}`;
    const observing = { matcher: '*', hooks: [{ type: 'command', command, timeout: 10 }] };
    await writeFile(join(home, 'hooks.json'), JSON.stringify({ hooks: {} }));
    const requests = [];
    const source = 'Write-Output ACME_WINDOWS_PROBE';
    let sources = [source];
    // Verify the observer's launch form independently before attributing an
    // absent receipt to host dispatch or configuration discovery.
    const probe = exec(process.env.ComSpec, ['/d', '/s', '/c', `"${observing.hooks[0].command}"`], {
      cwd: root, windowsVerbatimArguments: true, timeout: 10_000, maxBuffer: 65_536,
    });
    probe.child.stdin.end(JSON.stringify({ tool_input: { command: source } }));
    await probe;
    assert.equal(JSON.parse(await readFile(join(root, 'hook-envelope.json'), 'utf8')).source, source);
    await rm(join(root, 'hook-envelope.json'));
    await rm(join(root, 'observer-started'));
    await rm(join(root, 'hook-stdin.json'));
    let count = 0;
    const server = createServer(async (request, response) => {
      let bytes = '';
      for await (const chunk of request) { bytes += chunk; assert.ok(bytes.length <= 1_000_000); }
      const body = JSON.parse(bytes || '{}');
      requests.push(body);
      assert.ok(requests.length <= 8);
      console.error(`Codex fixture: provider response ${requests.length}`);
      const name = body.tools?.find(tool => ['exec_command', 'shell_command'].includes(tool.name))?.name;
      assert.ok(name, 'the native host must advertise a supported shell tool');
      const next = sources[count];
      const args = name === 'exec_command' ? { cmd: next, workdir: root } : { command: next, workdir: root };
      const call = count++;
      const item = next !== undefined
        ? { type: 'function_call', id: `fc_acme_${call}`, call_id: `call_acme_${call}`, name, arguments: JSON.stringify(args) }
        : { type: 'message', id: 'msg_acme', role: 'assistant', status: 'completed', content: [{ type: 'output_text', text: 'Acme fixture finished.', annotations: [] }] };
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
    await new Promise((resolve, reject) => { server.once('error', reject); server.listen(0, '127.0.0.1', resolve); });
    cleanup.push(() => new Promise(resolve => { server.closeAllConnections(); server.close(resolve); }));
    await writeFile(join(home, 'config.toml'), `model = "acme-model"\nmodel_provider = "acme"\n[model_providers.acme]\nname = "Acme local fixture"\nbase_url = "http://127.0.0.1:${server.address().port}/v1"\nwire_api = "responses"\nrequires_openai_auth = false\n[projects.${JSON.stringify(root)}]\ntrust_level = "trusted"\n`);
    try {
      await writeFile(hooksPath, JSON.stringify({ hooks: { PreToolUse: [observing] } }));
      const run = async () => {
        const running = exec(process.execPath, [executable, '--ask-for-approval', 'never', 'exec', '--sandbox', 'danger-full-access', '--enable', 'hooks', '--enable', 'unified_exec', '--disable', 'remote_plugin', '--dangerously-bypass-hook-trust', '--json', '-C', root, 'Perform the Acme fixture command.'], {
        cwd: root, env: { ...process.env, CODEX_HOME: home }, timeout: 45_000, maxBuffer: 2_000_000,
      });
        running.child.stdin.end();
        return running;
      };
      const result = await run();
      const outputs = requests.flatMap(request => request.input ?? []).filter(item => item.type === 'function_call_output').map(item => item.output).join('\n');
      assert.match(outputs, /ACME_WINDOWS_PROBE/);
      assert.doesNotMatch(outputs + result.stderr, /Hook failed/i);
      const envelope = JSON.parse(await readFile(join(root, 'hook-envelope.json'), 'utf8').catch(async error => {
        throw new Error(`Native Codex hook observer receipt missing: ${error.code}; started=${await readFile(join(root, 'observer-started'), 'utf8').catch(() => 'absent')}; stdin=${await readFile(join(root, 'hook-stdin.json'), 'utf8').catch(() => 'absent')}\n${result.stdout}\n${result.stderr}`);
      }));
      assert.equal(envelope.source, source);
      // Observer discovery is established above. Enforce with exactly the
      // generated project hooks, without an extra observer process per call.
      await writeFile(hooksPath, JSON.stringify(hooks));
      await writeFile(join(home, 'hooks.json'), JSON.stringify({ hooks: {} }));
      count = 0;
      requests.length = 0;
      const prefixAsk = 'git config --local acme.allowed present --add';
      sources = ['git config --local acme.allowed present', 'git config --local acme.denied forbidden', 'git config --local acme.asked present', prefixAsk];
      console.error('Codex fixture: generated-hook enforcement');
      const enforced = await run();
      const withheld = requests.flatMap(request => request.input ?? []).filter(item => item.type === 'function_call_output').map(item => item.output).join('\n');
      assert.match(withheld + enforced.stderr, /Shield:/);
      const events = JSON.parse((await exec(shield, ['__internal', 'history-events', '--project', root])).stdout).events;
      assert.ok(events.some(event => event.kind === 'decision' && event.provenance.agent === 'codex'), 'the generated project hook must record a decision');
      assert.equal((await exec('git', ['-C', root, 'config', '--local', '--get-regexp', '^acme\\.'])).stdout.trim(), 'acme.allowed present\nacme.asked present');
      const decisions = events.filter(event => event.kind === 'decision' && event.provenance.agent === 'codex').map(event => event.details.decision);
      for (const decision of ['allow', 'deny', 'ask']) assert.ok(decisions.includes(decision), `recorded ${decision}: ${JSON.stringify(decisions)}`);
      const prefixEvent = events.find(event => event.kind === 'decision' && event.details.operation?.source === prefixAsk);
      assert.equal(prefixEvent?.details.decision, 'ask');
      assert.equal(prefixEvent?.details.host_decision, 'deny');
      await writeFile(join(root, 'agent-probe.json'), JSON.stringify({ agent, observerDispatched: true, allowed: true, denied: true, askFallbackApplied: true, prefixCoveredAskWithheld: true, envelope }));
    } finally { await writeFile(hooksPath, original); }
  }
} finally {
  for (const close of cleanup.reverse()) await close();
}
