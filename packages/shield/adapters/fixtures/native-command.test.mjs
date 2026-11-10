import test from 'node:test';
import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { once } from 'node:events';
import { mkdtemp, readFile, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { setTimeout as pause } from 'node:timers/promises';
import { command } from './native-command.mjs';

const target = fileURLToPath(new URL('./native-command-target.mjs', import.meta.url));
const controller = fileURLToPath(new URL('./native-command.py', import.meta.url));
const unix = process.platform !== 'win32';
async function gone(pid) {
  for (let attempt = 0; attempt < 200; attempt++) {
    try { process.kill(pid, 0); } catch (error) { if (error.code === 'ESRCH') return; throw error; }
    await pause(10);
  }
  assert.fail(`Owned local process ${pid} remained after cleanup`);
}
async function fixture(t) {
  const root = await mkdtemp(join(tmpdir(), 'shield-owned-native-'));
  t.after(() => rm(root, { recursive: true, force: true }));
  return { path: join(root, 'pid.json'), root };
}
async function pids(path) {
  for (let attempt = 0; attempt < 200; attempt++) {
    try { return JSON.parse(await readFile(path, 'utf8')); } catch (error) { if (error.code !== 'ENOENT') throw error; }
    await pause(10);
  }
  assert.fail('Owned local target did not become ready');
}

test('fast leader exit cleans a TERM-resistant same-group descendant without touching another child', { skip: !unix, timeout: 8000 }, async t => {
  const unrelated = spawn(process.execPath, [target, 'wait'], { env: {}, stdio: 'ignore' });
  const reaped = once(unrelated, 'exit');
  t.after(async () => { unrelated.kill('SIGKILL'); await reaped; });
  const result = await command(process.execPath, [target, 'leader-descendant'], { timeoutMs: 3000 });
  assert.equal(result.code, 0);
  const ids = JSON.parse(result.stdout);
  await gone(ids.leader);
  await gone(ids.descendant);
  assert.equal(unrelated.exitCode, null);
  assert.equal(unrelated.signalCode, null);
});

test('an exited leader with no live group members still reaps successfully', { skip: !unix, timeout: 5000 }, async () => {
  const result = await command(process.execPath, [target, 'exit'], { timeoutMs: 3000 });
  assert.equal(result.code, 0);
  assert.equal(result.signal, null);
  assert.equal(result.stdout, 'Acme fixture complete.\n');
});

for (const mode of ['timeout', 'abort', 'overflow']) test(`owned ${mode} waits for controller cleanup and reaping`, { skip: !unix, timeout: 8000 }, async t => {
  const f = await fixture(t), abort = new AbortController();
  const result = command(process.execPath, [target, mode === 'overflow' ? mode : 'wait'], {
    env: { FIXTURE_PID_FILE: f.path }, timeoutMs: mode === 'timeout' ? 800 : 3000, abortSignal: abort.signal });
  const rejected = assert.rejects(result, mode === 'timeout' ? /deadline/ : mode === 'abort' ? /cancelled/ : /output exceeded/);
  const ids = await pids(f.path);
  if (mode === 'abort') abort.abort();
  await rejected;
  await gone(ids.pid);
  await gone(ids.controller);
});

test('parent disconnect requests cleanup even without a surviving Node timeout', { skip: !unix, timeout: 8000 }, async t => {
  const f = await fixture(t);
  const parent = spawn(process.execPath, [target, 'parent-disconnect'], { env: { FIXTURE_PID_FILE: f.path, PATH: process.env.PATH ?? '/usr/bin:/bin' }, stdio: 'ignore' });
  const reaped = once(parent, 'exit');
  t.after(async () => {
    if (parent.exitCode === null && parent.signalCode === null) parent.kill('SIGKILL');
    await reaped;
  });
  const ids = await pids(f.path);
  assert.deepEqual(await reaped, [0, null]);
  await gone(ids.pid);
  await gone(ids.controller);
});

test('spawn failure and already-cancelled commands fail without a target', { skip: !unix, timeout: 5000 }, async () => {
  await assert.rejects(command('/fixture/missing-native-program', []), /controller/);
  const abort = new AbortController(); abort.abort();
  await assert.rejects(command(process.execPath, [target, 'wait'], { abortSignal: abort.signal }), /cancelled/);
});

for (const mode of ['backstop', 'exception']) test(`controller ${mode} cleans and reaps without the parent command timer`, { skip: !unix, timeout: 8000 }, async t => {
  const f = await fixture(t);
  const child = spawn('python3', [controller], { env: { PATH: process.env.PATH ?? '/usr/bin:/bin' }, stdio: ['pipe', 'ignore', 'ignore', 'pipe'] });
  const status = [];
  child.stdio[3].on('data', bytes => status.push(bytes));
  child.stdin.on('error', () => {});
  const closed = once(child, 'close');
  child.stdin.write(`${JSON.stringify({ program: process.execPath, args: [target, 'wait'], cwd: f.root,
    env: { FIXTURE_PID_FILE: f.path }, timeout_ms: mode === 'backstop' ? 800 : 3000 })}\n`);
  const ids = await pids(f.path);
  if (mode === 'exception') child.kill('SIGINT'); // Python finally still owns the unreaped target.
  await closed;
  child.stdin.destroy();
  const result = JSON.parse(Buffer.concat(status).toString('utf8'));
  assert.equal(result.reaped, true);
  assert.match(result.error, mode === 'backstop' ? /deadline/ : /controller failed/);
  await gone(ids.pid);
});
