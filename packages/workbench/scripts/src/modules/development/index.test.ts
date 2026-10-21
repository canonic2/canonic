import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { execFileSync } from 'node:child_process';
import { focused, main, prepare } from './index.ts';

test('focused execution runs only selected files and propagates real test failures', t => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'acme-focused-tests-'));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  fs.mkdirSync(path.join(root, 'ui'));
  fs.writeFileSync(path.join(root, 'ui/pass.test.js'), "require('node:test')('passes', () => {});");
  fs.writeFileSync(path.join(root, 'ui/fail.test.js'), "require('node:test')('fails', () => { throw new Error('fixture failure'); });");
  const io = {
    buildServer() { assert.fail('UI tests must not build the server'); },
    async bundleRuntime() { assert.fail('tests must not bundle Electron'); },
    run(command: string, args: string[], cwd: string) {
      const env = { ...process.env };
      delete env.NODE_TEST_CONTEXT;
      execFileSync(command, args, { cwd, env, stdio: 'pipe' });
    },
    log() {},
  };
  focused(['ui/pass.test.js'], undefined, io, root);
  assert.throws(() => focused(['ui/fail.test.js'], undefined, io, root));
});

test('focused tests skip Cargo for UI files and build before backend tests', t => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'acme-development-'));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  for (const file of ['ui/space name.test.js', 'server/native.test.ts']) {
    fs.mkdirSync(path.dirname(path.join(root, file)), { recursive: true });
    fs.writeFileSync(path.join(root, file), '');
  }
  const calls: unknown[] = [];
  const io = {
    buildServer() { calls.push('build'); },
    async bundleRuntime() { calls.push('runtime'); },
    run(command: string, args: string[], cwd: string) { calls.push({ command, args, cwd }); },
    log() {},
  };
  focused(['ui/space name.test.js'], 'selected behavior', io, root);
  assert.deepEqual(calls, [{ command: process.execPath, args: ['--test', '--test-concurrency=4', '--test-name-pattern=selected behavior', 'ui/space name.test.js'], cwd: root }]);
  calls.length = 0;
  focused(['server/native.test.ts'], undefined, io, root);
  assert.equal(calls[0], 'build');
  assert.equal(calls.length, 2);
  assert.throws(() => focused([], undefined, io, root), /Choose test files/);
  assert.throws(() => focused(['../outside.test.js'], undefined, io, root), /Expected a Workbench test file/);
  assert.throws(() => focused(['server/native.test.ts'], undefined, { ...io, buildServer() { throw new Error('compile failed'); } }, root), /compile failed/);
  assert.equal(calls.length, 2, 'failed builds never run stale tests');
});

test('development prepares source assets before launching and propagates preparation failures', async () => {
  const calls: unknown[] = [];
  const io = {
    buildServer() { calls.push('build'); },
    async bundleRuntime() { calls.push('runtime'); },
    run(command: string, args: string[], cwd: string) { calls.push({ command, args, cwd }); },
    log() {},
  };
  await prepare(io);
  assert.deepEqual(calls, ['build', 'runtime']);
  calls.length = 0;
  await main(['launch', '.'], io, process.cwd());
  assert.deepEqual(calls, ['build', 'runtime', { command: 'code', args: ['--new-window', '--extensionDevelopmentPath=' + path.join(process.cwd(), 'extension'), process.cwd()], cwd: process.cwd() }]);
  calls.length = 0;
  await assert.rejects(main(['launch', '.'], { ...io, async bundleRuntime() { throw new Error('runtime failed'); } }), /runtime failed/);
  assert.deepEqual(calls, ['build']);
});
