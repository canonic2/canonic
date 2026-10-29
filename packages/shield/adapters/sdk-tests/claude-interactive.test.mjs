import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtemp, realpath, rm, readFile, writeFile, mkdir } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { spawn, execFile } from 'node:child_process';
import { promisify } from 'node:util';
import { once } from 'node:events';
import { setTimeout as delay } from 'node:timers/promises';
import { binary } from '../fixtures/runtime.mjs';
import { scriptedProvider } from './claude-provider.mjs';
import { preparePolicyChange } from './policy-proposal.mjs';

const exec = promisify(execFile);
const helper = fileURLToPath(new URL('./claude-terminal.py', import.meta.url));
const initializer = fileURLToPath(new URL('../../tests/integration/operator_terminal.py', import.meta.url));
const quote = value => `'${value.replaceAll("'", "'\\''")}'`;
const unstyle = text => text.replace(/\x1b\][^\x07]*(?:\x07|\x1b\\)/g, '')
  .replace(/\x1b\[[0-?]*[ -/]*[@-~]/g, '');

// Real interactive CLI, ordinary renderer, no SDK elicitation callback or hook.
// The scripted provider chooses the tool; only terminal input answers its form.
for (const mode of ['auto', 'bypassPermissions']) {
  for (const action of ['accept', 'decline', 'cancel']) {
    test(`interactive Claude ${mode}: policy form ${action}`, { timeout: 60000 }, async t => {
      const root = await realpath(await mkdtemp(join(tmpdir(), 'shield-interactive-acme-')));
      t.after(() => rm(root, { recursive: true, force: true }));
      const initialized = JSON.parse((await exec('python3', [initializer, binary, root, 'agents'],
        { timeout: 35000 })).stdout);
      assert.equal(initialized.code, 0);
      assert.equal(initialized.activated, true);
      const args = await preparePolicyChange(root, 'claude');
      const statePath = join(root, '.shield/review/state.json');
      const state = async () => JSON.parse(await readFile(statePath, 'utf8'));
      const before = await state();
      const provider = await scriptedProvider(t, [{ name: 'mcp__shield__policy_apply', input: args }]);
      const config = join(root, 'claude-config');
      await mkdir(config);
      const metadata = { hasCompletedOnboarding: true, theme: 'dark', bypassPermissionsModeAccepted: true,
        customApiKeyResponses: { approved: ['acme-local-test-key'], rejected: [] },
        projects: { [root]: { hasTrustDialogAccepted: true, hasCompletedProjectOnboarding: true } } };
      await writeFile(join(config, '.claude.json'), JSON.stringify(metadata));
      await writeFile(join(root, '.claude.json'), JSON.stringify(metadata));
      const observer = join(root, 'observer.mjs');
      const observed = join(root, 'mode.json');
      // Observe the host's actual mode without changing its permission decision.
      await writeFile(observer, `import {writeFileSync} from 'node:fs';
let b=''; for await(const c of process.stdin)b+=c; const h=JSON.parse(b);
writeFileSync(${JSON.stringify(observed)},JSON.stringify({mode:h.permission_mode,tool:h.tool_name}));`);
      const settingsPath = join(root, '.claude/settings.local.json');
      const settings = JSON.parse(await readFile(settingsPath, 'utf8'));
      settings.hooks.PreToolUse.push({ matcher: 'mcp__shield__policy_apply',
        hooks: [{ type: 'command', command: `${quote(process.execPath)} ${quote(observer)}` }] });
      await writeFile(settingsPath, JSON.stringify(settings));
      const claude = (await exec('which', ['claude'])).stdout.trim();
      t.diagnostic((await exec(claude, ['--version'])).stdout.trim());
      const child = spawn('python3', [helper], { stdio: ['pipe', 'pipe', 'pipe'] });
      const closed = once(child, 'close');
      let transcript = '', pending = '', errors = '', exited = false;
      child.on('close', () => { exited = true; });
      child.stdout.on('data', bytes => {
        pending += bytes;
        let index;
        while ((index = pending.indexOf('\n')) !== -1) {
          const event = JSON.parse(pending.slice(0, index));
          pending = pending.slice(index + 1);
          transcript += event.text ?? '';
        }
      });
      child.stderr.on('data', bytes => { errors += bytes; });
      const send = message => child.stdin.write(`${JSON.stringify(message)}\n`);
      const wait = async condition => {
        const deadline = Date.now() + 20000;
        while (!await condition()) {
          assert.ok(!exited && Date.now() < deadline && !t.signal.aborted,
            `Interactive terminal stopped or timed out: ${errors}\n${unstyle(transcript).slice(-7000)}`);
          await delay(25);
        }
      };
      send({ argv: [claude, '--permission-mode', mode, '--allow-dangerously-skip-permissions',
        '--model', 'claude-opus-4-6', '--setting-sources', 'project,local', 'Perform the Acme policy fixture.'],
        cwd: root, env: { ...process.env, HOME: root, CLAUDE_CONFIG_DIR: config,
          ANTHROPIC_API_KEY: 'acme-local-test-key', ANTHROPIC_BASE_URL: provider.url,
          CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC: '1', TERM: 'xterm-256color', NO_COLOR: '1' } });
      try {
        await wait(() => unstyle(transcript).replace(/\s/g, '').includes('ApprovethisShieldpolicychange?'));
        await wait(() => unstyle(transcript).includes('Decline'));
        assert.equal(JSON.parse(await readFile(observed, 'utf8')).mode, mode);
        // A visible form must remain pending without keyboard input in both modes.
        await delay(500);
        assert.deepEqual((await state()).active, before.active);
        assert.equal((await state()).policy_revision, before.policy_revision);
        assert.equal(provider.results.length, 0);
        if (action === 'decline') {
          send({ input: '\x1b[B' });
          await delay(200);
        }
        send({ input: action === 'cancel' ? '\x1b' : '\r' });
        await wait(() => provider.results.length > 0);
        const after = await state();
        assert.equal(after.proposals.at(-1).status, action === 'accept' ? 'applied' : 'pending');
        if (action === 'accept') assert.notEqual(after.policy_revision, before.policy_revision);
        else {
          assert.equal(after.policy_revision, before.policy_revision);
          assert.deepEqual(after.active, before.active);
        }
      } finally {
        if (!exited) send({ stop: true });
        await closed;
      }
    });
  }
}
